//! Claude Code adapter: reads `<root>/<project>/*.jsonl` session logs, plus
//! each session's `<project>/<session>/subagents/` records (SPEC.md, stage 4).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Utc};

use super::subagent::{self, SubagentRun};
use super::{CollectContext, CollectedSession, SourceAdapter, SourceCollection, SourceFingerprint};
use crate::analyzer::prompt::TimedLine;
use crate::parser::claude::{ContentBlock, LogEntry, SubagentToolResult};
use crate::{analyzer, config, parser};

/// Exclusion reason for sessions started by SDK runs or hooks.
pub const EXCLUDED_AUTOMATED: &str = "automated";

#[derive(Debug, Default)]
pub struct ClaudeAdapter {
    /// Fixed roots for tests; `None` discovers roots from config and home.
    roots: Option<Vec<PathBuf>>,
}

impl ClaudeAdapter {
    pub const ID: &'static str = "claude";

    /// Reads only the given roots, skipping config overrides and home discovery.
    #[cfg(test)]
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots: Some(roots) }
    }

    fn resolve_roots(&self, config: Option<&config::Config>) -> Vec<PathBuf> {
        match &self.roots {
            Some(roots) => parser::roots::dedupe_existing_paths(roots.clone()),
            None => parser::discover_claude_log_roots(root_overrides(config)),
        }
    }
}

fn root_overrides(config: Option<&config::Config>) -> Option<&[String]> {
    config.and_then(|cfg| cfg.input.as_ref()).and_then(|input| {
        input
            .claude
            .as_ref()
            .and_then(|claude| claude.roots.as_deref())
            .or(input.claude_roots.as_deref())
    })
}

fn include_automated_from_config(config: Option<&config::Config>) -> bool {
    config
        .and_then(|cfg| cfg.input.as_ref())
        .and_then(|input| input.claude.as_ref())
        .map(|claude| claude.include_automated)
        .unwrap_or(false)
}

impl SourceAdapter for ClaudeAdapter {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn collect(&self, ctx: &CollectContext) -> Result<SourceCollection, parser::ParseError> {
        let include_automated = ctx.include_automated || include_automated_from_config(ctx.config);
        let roots = self.resolve_roots(ctx.config);

        let mut all_project_dirs = Vec::new();
        let mut file_count = 0;
        let mut total_entries = 0;
        let mut all_entries = Vec::new();
        for root in &roots {
            let Ok(project_dirs) = parser::list_project_dirs_in_root(root) else {
                continue;
            };
            for project_dir in project_dirs {
                let Ok(session_files) = parser::list_session_files(&project_dir) else {
                    all_project_dirs.push(project_dir);
                    continue;
                };
                file_count += session_files.len();
                for session_file in session_files {
                    if let Ok(entries) = parser::parse_jsonl_file(&session_file) {
                        total_entries += entries.len();
                        all_entries.extend(parser::filter_entries_by_date(entries, ctx.date));
                    }
                }
                all_project_dirs.push(project_dir);
            }
        }
        let project_count = all_project_dirs.len();

        let deduped = parser::dedupe_claude_entries(all_entries);
        let filtered = parser::filter_automated_sessions(deduped, include_automated);
        let counts = filtered.counts;
        let mut entries = filtered.entries;
        entries.sort_by(|left, right| {
            match (
                parser::claude::entry_timestamp(left),
                parser::claude::entry_timestamp(right),
            ) {
                (Some(l), Some(r)) => l.cmp(&r),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        let runs_by_session = collect_subagent_runs(&all_project_dirs, &entries, ctx.date);
        let attached_subagents = runs_by_session.values().map(Vec::len).sum();
        let lines_by_session: HashMap<String, Vec<TimedLine>> = runs_by_session
            .iter()
            .map(|(session_id, runs)| (session_id.clone(), subagent::subagent_lines(runs)))
            .collect();

        let sessions = build_sessions(&entries, &lines_by_session)?;
        let excluded_automated = if include_automated {
            0
        } else {
            counts.automated
        };

        Ok(SourceCollection {
            roots,
            fingerprint: SourceFingerprint {
                session_count: sessions.len(),
                entry_count: entries.len(),
            },
            sessions,
            excluded: vec![(EXCLUDED_AUTOMATED, excluded_automated)],
            verbose_lines: vec![
                crate::messages::verbose::discover_stats(
                    project_count,
                    file_count,
                    total_entries,
                    entries.len(),
                ),
                crate::messages::verbose::claude_session_mix(
                    counts.interactive,
                    counts.automated,
                    include_automated,
                ),
                crate::messages::verbose::subagents(self.display_name(), attached_subagents, 0),
            ],
        })
    }
}

/// Folder name, under a session folder, that holds subagent record files.
const SUBAGENTS_DIR: &str = "subagents";
/// Names used when the parent log has no `agentType` (SPEC.md, stage 4).
const SUBAGENT_NAME_WORKFLOW: &str = "workflow";
const SUBAGENT_NAME_DEFAULT: &str = "subagent";

/// Gathers each session's subagent runs for the target date.
///
/// Record files under `<project>/<session>/subagents/` come first: the task is
/// the first user text and the result is the last assistant text. A run sits
/// where the parent got its tool result (same `agentId`), or at the record's
/// first timestamp when the parent log has none (workflow runs). Parent tool
/// results without any record file fall back to their `prompt` and `content`.
fn collect_subagent_runs(
    project_dirs: &[PathBuf],
    entries: &[LogEntry],
    date: NaiveDate,
) -> HashMap<String, Vec<SubagentRun>> {
    let mut session_order: Vec<&str> = Vec::new();
    let mut links: HashMap<&str, Vec<(DateTime<Utc>, SubagentToolResult)>> = HashMap::new();
    for entry in entries {
        let Some(session_id) = entry_session_id(entry) else {
            continue;
        };
        if !links.contains_key(session_id) {
            session_order.push(session_id);
            links.insert(session_id, Vec::new());
        }
        if let LogEntry::User(user) = entry
            && let Some(link) = parser::claude::subagent_tool_result(user)
            && let Some(session_links) = links.get_mut(session_id)
            && !session_links
                .iter()
                .any(|(_, known)| known.agent_id == link.agent_id)
        {
            session_links.push((user.timestamp, link));
        }
    }

    let mut runs_by_session = HashMap::new();
    for session_id in session_order {
        let session_links = links.get(session_id).map(Vec::as_slice).unwrap_or_default();
        let link_for = |agent_id: &str| {
            session_links
                .iter()
                .find(|(_, link)| link.agent_id == agent_id)
        };

        let mut seen_agents: HashSet<String> = HashSet::new();
        let mut runs = Vec::new();
        for project_dir in project_dirs {
            let dir = project_dir.join(session_id).join(SUBAGENTS_DIR);
            for (path, is_workflow) in parser::claude::list_subagent_files(&dir) {
                let Some(agent_id) = agent_id_from_record_path(&path) else {
                    continue;
                };
                // The same session can exist under several roots.
                if !seen_agents.insert(agent_id.to_string()) {
                    continue;
                }
                let Ok(record_entries) = parser::parse_jsonl_file(&path) else {
                    continue;
                };
                let record = read_subagent_record(&record_entries);
                let link = link_for(agent_id);
                let Some(at) = link.map(|(ts, _)| *ts).or(record.first_activity) else {
                    continue;
                };
                let fallback_name = if is_workflow {
                    SUBAGENT_NAME_WORKFLOW
                } else {
                    SUBAGENT_NAME_DEFAULT
                };
                runs.push(SubagentRun {
                    name: link
                        .and_then(|(_, link)| link.agent_type.clone())
                        .unwrap_or_else(|| fallback_name.to_string()),
                    task: record.task,
                    result: record.result,
                    at,
                });
            }
        }
        for (ts, link) in session_links {
            if seen_agents.contains(&link.agent_id) {
                continue;
            }
            runs.push(SubagentRun {
                name: link
                    .agent_type
                    .clone()
                    .unwrap_or_else(|| SUBAGENT_NAME_DEFAULT.to_string()),
                task: link.prompt.clone(),
                result: link.content.clone(),
                at: *ts,
            });
        }

        let runs = subagent::keep_target_date(runs, date);
        if !runs.is_empty() {
            runs_by_session.insert(session_id.to_string(), runs);
        }
    }
    runs_by_session
}

/// `.../agent-<agentId>.jsonl` -> `<agentId>`.
fn agent_id_from_record_path(path: &Path) -> Option<&str> {
    path.file_stem()?
        .to_str()?
        .strip_prefix(parser::claude::SUBAGENT_FILE_PREFIX)
        .filter(|id| !id.is_empty())
}

/// What one subagent record file says.
struct SubagentRecord {
    task: String,
    result: String,
    first_activity: Option<DateTime<Utc>>,
}

/// Task = first user entry's text; result = text of the last assistant entry
/// that has any (thinking and tool calls are skipped). Verified: all 63 record
/// files start with a user entry whose content is the task string.
fn read_subagent_record(entries: &[LogEntry]) -> SubagentRecord {
    let task = entries
        .iter()
        .find_map(|entry| match entry {
            LogEntry::User(user) => Some(user.message.as_ref().map(user_text).unwrap_or_default()),
            _ => None,
        })
        .unwrap_or_default();
    let result = entries
        .iter()
        .rev()
        .find_map(|entry| match entry {
            LogEntry::Assistant(assistant) => {
                let text = assistant
                    .message
                    .as_ref()
                    .map(|message| assistant_text(&message.content))
                    .unwrap_or_default();
                (!text.trim().is_empty()).then_some(text)
            }
            _ => None,
        })
        .unwrap_or_default();
    SubagentRecord {
        task,
        result,
        first_activity: entries
            .iter()
            .filter_map(parser::claude::entry_timestamp)
            .min(),
    }
}

/// Text the user typed: a string `content` or its `text` blocks.
fn user_text(message: &serde_json::Value) -> String {
    match message.get("content") {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(serde_json::Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn assistant_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text: Some(text) } if !text.trim().is_empty() => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn entry_session_id(entry: &LogEntry) -> Option<&str> {
    match entry {
        LogEntry::User(e) => Some(&e.session_id),
        LogEntry::Assistant(e) => Some(&e.session_id),
        LogEntry::Progress(e) => Some(&e.session_id),
        LogEntry::System(e) => e.session_id.as_deref(),
        LogEntry::FileHistorySnapshot(_) | LogEntry::Other(_) => None,
    }
}

/// Builds one `CollectedSession` per Claude session, in first-activity order.
fn build_sessions(
    entries: &[LogEntry],
    lines_by_session: &HashMap<String, Vec<TimedLine>>,
) -> Result<Vec<CollectedSession>, parser::ParseError> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let tasks = analyzer::build_claude_session_tasks(entries, ClaudeAdapter::ID, lines_by_session)?;

    let mut activity: HashMap<&str, (DateTime<Utc>, DateTime<Utc>)> = HashMap::new();
    for entry in entries {
        let (Some(session_id), Some(ts)) = (
            entry_session_id(entry),
            parser::claude::entry_timestamp(entry),
        ) else {
            continue;
        };
        activity
            .entry(session_id)
            .and_modify(|(first, last)| {
                *first = (*first).min(ts);
                *last = (*last).max(ts);
            })
            .or_insert((ts, ts));
    }

    let tokens: HashMap<String, (u64, u64)> = parser::summarize_entries(entries)
        .into_iter()
        .map(|s| {
            let total_in =
                s.total_input_tokens + s.total_cache_creation_tokens + s.total_cache_read_tokens;
            (s.session_id, (total_in, s.total_output_tokens))
        })
        .collect();

    Ok(tasks
        .into_iter()
        .map(|task| {
            let range = activity.get(task.session_id.as_str()).copied();
            let token_usage = tokens.get(&task.session_id).copied().unwrap_or((0, 0));
            CollectedSession {
                first_activity: range.map(|(first, _)| first),
                last_activity: range.map(|(_, last)| last),
                token_usage: Some(token_usage),
                task,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ))
    }

    fn date() -> chrono::NaiveDate {
        // Far-future date so nothing collides with real logs.
        chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date")
    }

    fn ctx(include_automated: bool) -> CollectContext<'static> {
        CollectContext {
            date: date(),
            config: None,
            include_automated,
        }
    }

    fn write_lines(path: &std::path::Path, lines: &[&str]) {
        std::fs::write(path, format!("{}\n", lines.join("\n"))).expect("write jsonl");
    }

    /// Writes one interactive session and one SDK-driven automated session.
    fn write_sample(root: &std::path::Path) {
        let project = root.join("project-a");
        std::fs::create_dir_all(&project).expect("project dir");
        write_lines(
            &project.join("interactive.jsonl"),
            &[
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2099-01-01T12:00:00Z","uuid":"u1","message":{"role":"user","content":"Why does the cache miss?"}}"#,
                r#"{"type":"assistant","sessionId":"claude-s1","timestamp":"2099-01-01T12:00:30Z","uuid":"a1","message":{"role":"assistant","content":[{"type":"thinking","thinking":"Check the key."},{"type":"text","text":"The timezone key changed."},{"type":"tool_use","name":"Read","id":"t1","input":{"file_path":"cache.rs"}}],"usage":{"input_tokens":100,"output_tokens":20,"cache_creation_input_tokens":5,"cache_read_input_tokens":7}}}"#,
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2099-01-01T12:05:00Z","uuid":"u2","message":{"role":"user","content":"Fix it."}}"#,
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2098-12-31T12:00:00Z","uuid":"u0","message":{"role":"user","content":"previous day"}}"#,
            ],
        );
        write_lines(
            &project.join("automated.jsonl"),
            &[
                r#"{"type":"user","sessionId":"claude-auto","timestamp":"2099-01-01T13:00:00Z","uuid":"u3","entrypoint":"sdk-cli","message":{"content":"Run exactly one Bash command"}}"#,
                r#"{"type":"assistant","sessionId":"claude-auto","timestamp":"2099-01-01T13:00:10Z","uuid":"a3","entrypoint":"sdk-cli","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#,
            ],
        );
    }

    #[test]
    fn test_collect_dedupes_entries_across_roots() {
        let base = unique_temp_dir("rwd_test_sources_claude_multi_root");
        let root_a = base.join("claude-a");
        let root_b = base.join("claude-b");
        std::fs::create_dir_all(root_a.join("project-a")).expect("project a");
        std::fs::create_dir_all(root_b.join("project-b")).expect("project b");
        let line = r#"{"type":"user","sessionId":"claude-session-1","timestamp":"2099-01-01T12:00:00Z","uuid":"same-entry","message":{"role":"user","content":"hello"}}"#;
        write_lines(&root_a.join("project-a/session-a.jsonl"), &[line]);
        write_lines(&root_b.join("project-b/session-b.jsonl"), &[line]);

        let adapter = ClaudeAdapter::with_roots(vec![root_a.clone(), root_b.clone()]);
        let collection = adapter.collect(&ctx(false)).expect("collect");
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.roots, vec![root_a, root_b]);
        assert_eq!(collection.sessions.len(), 1);
        assert_eq!(collection.sessions[0].task.session_id, "claude-session-1");
        assert_eq!(collection.fingerprint.entry_count, 1);
    }

    #[test]
    fn test_collect_sample_builds_fixed_prompt_text() {
        let base = unique_temp_dir("rwd_test_sources_claude_fixed_prompt");
        write_sample(&base);

        let collection = ClaudeAdapter::with_roots(vec![base.clone()])
            .collect(&ctx(false))
            .expect("collect");
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.sessions.len(), 1);
        let task = &collection.sessions[0].task;
        assert_eq!(task.session_id, "claude-s1");
        assert_eq!(
            task.prompt_text,
            "[Session: claude-s1]\n\
             [USER] Why does the cache miss?\n\
             [ASSISTANT] Check the key.\nThe timezone key changed.\n\
             [tool: Read]\n\
             [USER] Fix it.\n\n"
        );
    }

    #[test]
    fn test_collect_excludes_automated_sessions_by_default() {
        let base = unique_temp_dir("rwd_test_sources_claude_automated");
        write_sample(&base);

        let collection = ClaudeAdapter::with_roots(vec![base.clone()])
            .collect(&ctx(false))
            .expect("collect");
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.excluded, vec![(EXCLUDED_AUTOMATED, 1)]);
        assert_eq!(
            collection.fingerprint,
            SourceFingerprint {
                session_count: 1,
                entry_count: 3,
            }
        );
    }

    #[test]
    fn test_collect_includes_automated_sessions_with_flag() {
        let base = unique_temp_dir("rwd_test_sources_claude_include_automated");
        write_sample(&base);

        let collection = ClaudeAdapter::with_roots(vec![base.clone()])
            .collect(&ctx(true))
            .expect("collect");
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.excluded, vec![(EXCLUDED_AUTOMATED, 0)]);
        let ids: Vec<&str> = collection
            .sessions
            .iter()
            .map(|s| s.task.session_id.as_str())
            .collect();
        assert_eq!(ids, vec!["claude-s1", "claude-auto"]);
        assert_eq!(collection.fingerprint.entry_count, 5);
    }

    #[test]
    fn test_collect_fills_activity_range_and_token_usage() {
        let base = unique_temp_dir("rwd_test_sources_claude_activity");
        write_sample(&base);

        let collection = ClaudeAdapter::with_roots(vec![base.clone()])
            .collect(&ctx(false))
            .expect("collect");
        std::fs::remove_dir_all(&base).ok();

        let session = &collection.sessions[0];
        let ts = |s: &str| s.parse::<DateTime<Utc>>().expect("timestamp");
        assert_eq!(session.first_activity, Some(ts("2099-01-01T12:00:00Z")));
        assert_eq!(session.last_activity, Some(ts("2099-01-01T12:05:00Z")));
        assert_eq!(session.token_usage, Some((112, 20)));
    }

    /// Parent session that ran one sync Explore agent (`a1`) and an async
    /// agent (`a2`), plus one agent (`a3`) with no record file.
    fn write_parent_with_subagents(root: &std::path::Path) -> PathBuf {
        let project = root.join("project-a");
        std::fs::create_dir_all(&project).expect("project dir");
        write_lines(
            &project.join("claude-s1.jsonl"),
            &[
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2099-01-01T12:00:00Z","uuid":"u1","message":{"role":"user","content":"Investigate the cache"}}"#,
                r#"{"type":"assistant","sessionId":"claude-s1","timestamp":"2099-01-01T12:00:30Z","uuid":"a1","message":{"role":"assistant","content":[{"type":"tool_use","name":"Agent","id":"t1","input":{"prompt":"PARENT_TOOL_INPUT"}}]}}"#,
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2099-01-01T12:03:00Z","uuid":"u2","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"PARENT_TOOL_OUTPUT"}]},"toolUseResult":{"status":"completed","agentId":"a1","agentType":"Explore","prompt":"PARENT_PROMPT_COPY","content":[{"type":"text","text":"PARENT_CONTENT_COPY"}]}}"#,
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2099-01-01T12:10:00Z","uuid":"u3","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t2","content":"launched"}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"a2","prompt":"PARENT_ASYNC_PROMPT"}}"#,
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2099-01-01T12:20:00Z","uuid":"u4","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t3","content":"x"}]},"toolUseResult":{"status":"completed","agentId":"a3","agentType":"Plan","prompt":"FALLBACK_TASK","content":[{"type":"text","text":"FALLBACK_RESULT"}]}}"#,
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2099-01-01T12:30:00Z","uuid":"u5","message":{"role":"user","content":"Thanks"}}"#,
            ],
        );
        let subagents = project.join("claude-s1").join("subagents");
        std::fs::create_dir_all(&subagents).expect("subagents dir");
        write_lines(
            &subagents.join("agent-a1.jsonl"),
            &[
                r#"{"type":"user","sessionId":"claude-s1","agentId":"a1","isSidechain":true,"timestamp":"2099-01-01T12:00:40Z","uuid":"s1","message":{"role":"user","content":"Find where the cache key is built"}}"#,
                r#"{"type":"assistant","sessionId":"claude-s1","agentId":"a1","isSidechain":true,"timestamp":"2099-01-01T12:01:00Z","uuid":"s2","message":{"role":"assistant","content":[{"type":"text","text":"SUBAGENT_MIDDLE_TEXT"},{"type":"tool_use","name":"Grep","id":"g1","input":{"pattern":"TOOL_INPUT_MARKER"}}]}}"#,
                r#"{"type":"user","sessionId":"claude-s1","agentId":"a1","isSidechain":true,"timestamp":"2099-01-01T12:01:10Z","uuid":"s3","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"g1","content":"TOOL_OUTPUT_MARKER"}]}}"#,
                r#"{"type":"assistant","sessionId":"claude-s1","agentId":"a1","isSidechain":true,"timestamp":"2099-01-01T12:02:00Z","uuid":"s4","message":{"role":"assistant","content":[{"type":"thinking","thinking":"SUBAGENT_THINKING"},{"type":"text","text":"The key is built in cache.rs"}]}}"#,
            ],
        );
        write_lines(
            &subagents.join("agent-a2.jsonl"),
            &[
                r#"{"type":"user","sessionId":"claude-s1","agentId":"a2","isSidechain":true,"timestamp":"2099-01-01T12:10:01Z","uuid":"r1","message":{"role":"user","content":"Run the slow suite"}}"#,
                r#"{"type":"assistant","sessionId":"claude-s1","agentId":"a2","isSidechain":true,"timestamp":"2099-01-01T12:15:00Z","uuid":"r2","message":{"role":"assistant","content":[{"type":"text","text":"All 12 tests passed"}]}}"#,
            ],
        );
        subagents
    }

    #[test]
    fn test_collect_splices_subagent_record_at_parent_tool_result() {
        let base = unique_temp_dir("rwd_test_sources_claude_subagent_record");
        write_parent_with_subagents(&base);

        let collection = ClaudeAdapter::with_roots(vec![base.clone()])
            .collect(&ctx(false))
            .expect("collect");
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.sessions.len(), 1);
        let prompt = &collection.sessions[0].task.prompt_text;
        assert_eq!(
            prompt,
            "[Session: claude-s1]\n\
             [USER] Investigate the cache\n\
             [ASSISTANT] [tool: Agent]\n\
             [USER] [tool_result]\n\
             [SUBAGENT] Explore: Find where the cache key is built\n\
             [SUBAGENT_RESULT] The key is built in cache.rs\n\
             [USER] [tool_result]\n\
             [SUBAGENT] subagent: Run the slow suite\n\
             [SUBAGENT_RESULT] All 12 tests passed\n\
             [USER] [tool_result]\n\
             [SUBAGENT] Plan: FALLBACK_TASK\n\
             [SUBAGENT_RESULT] FALLBACK_RESULT\n\
             [USER] Thanks\n\n"
        );
        for marker in [
            "TOOL_INPUT_MARKER",
            "TOOL_OUTPUT_MARKER",
            "SUBAGENT_MIDDLE_TEXT",
            "SUBAGENT_THINKING",
            "PARENT_PROMPT_COPY",
            "PARENT_CONTENT_COPY",
            "PARENT_ASYNC_PROMPT",
        ] {
            assert!(!prompt.contains(marker), "{marker} leaked:\n{prompt}");
        }
    }

    #[test]
    fn test_collect_subagents_stay_out_of_session_count_and_fingerprint() {
        let base = unique_temp_dir("rwd_test_sources_claude_subagent_count");
        write_parent_with_subagents(&base);

        let collection = ClaudeAdapter::with_roots(vec![base.clone()])
            .collect(&ctx(false))
            .expect("collect");
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(
            collection.fingerprint,
            SourceFingerprint {
                session_count: 1,
                entry_count: 6,
            }
        );
        assert!(
            collection
                .verbose_lines
                .contains(&"[discover] Claude Code subagents: 3 attached".to_string()),
            "{:?}",
            collection.verbose_lines
        );
        let task = &collection.sessions[0].task;
        let user_count = task.messages.iter().filter(|(r, _)| r == "USER").count();
        assert_eq!(user_count, 5);
    }

    #[test]
    fn test_collect_places_workflow_record_at_its_first_timestamp() {
        let base = unique_temp_dir("rwd_test_sources_claude_subagent_workflow");
        let subagents = write_parent_with_subagents(&base);
        let workflow = subagents.join("workflows").join("wf_1");
        std::fs::create_dir_all(&workflow).expect("workflow dir");
        write_lines(
            &workflow.join("agent-w1.jsonl"),
            &[
                r#"{"type":"user","sessionId":"claude-s1","agentId":"w1","isSidechain":true,"timestamp":"2099-01-01T12:25:00Z","uuid":"w1","message":{"role":"user","content":"Review step"}}"#,
                r#"{"type":"assistant","sessionId":"claude-s1","agentId":"w1","isSidechain":true,"timestamp":"2099-01-01T12:26:00Z","uuid":"w2","message":{"role":"assistant","content":[{"type":"text","text":"Looks fine"},{"type":"tool_use","name":"StructuredOutput","id":"o1","input":{"verdict":"STRUCTURED_OUTPUT_MARKER"}}]}}"#,
            ],
        );
        // A record from the previous day stays out of today's prompt.
        write_lines(
            &subagents.join("agent-old.jsonl"),
            &[
                r#"{"type":"user","sessionId":"claude-s1","agentId":"old","isSidechain":true,"timestamp":"2098-12-31T12:00:00Z","uuid":"o1","message":{"role":"user","content":"YESTERDAY_TASK"}}"#,
            ],
        );

        let collection = ClaudeAdapter::with_roots(vec![base.clone()])
            .collect(&ctx(false))
            .expect("collect");
        std::fs::remove_dir_all(&base).ok();

        let prompt = &collection.sessions[0].task.prompt_text;
        assert!(
            prompt.contains(
                "[SUBAGENT_RESULT] FALLBACK_RESULT\n\
                 [SUBAGENT] workflow: Review step\n\
                 [SUBAGENT_RESULT] Looks fine\n\
                 [USER] Thanks\n"
            ),
            "{prompt}"
        );
        assert!(!prompt.contains("STRUCTURED_OUTPUT_MARKER"));
        assert!(!prompt.contains("YESTERDAY_TASK"));
    }

    #[test]
    fn test_collect_missing_root_returns_empty_collection() {
        let base = unique_temp_dir("rwd_test_sources_claude_missing");

        let collection = ClaudeAdapter::with_roots(vec![base])
            .collect(&ctx(false))
            .expect("collect");

        assert!(collection.roots.is_empty());
        assert!(collection.sessions.is_empty());
        assert_eq!(collection.fingerprint, SourceFingerprint::default());
    }
}
