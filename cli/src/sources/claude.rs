//! Claude Code adapter: reads `<root>/<project>/*.jsonl` session logs.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};

use super::{CollectContext, CollectedSession, SourceAdapter, SourceCollection, SourceFingerprint};
use crate::parser::claude::LogEntry;
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

        let mut project_count = 0;
        let mut file_count = 0;
        let mut total_entries = 0;
        let mut all_entries = Vec::new();
        for root in &roots {
            let Ok(project_dirs) = parser::list_project_dirs_in_root(root) else {
                continue;
            };
            project_count += project_dirs.len();
            for project_dir in project_dirs {
                let Ok(session_files) = parser::list_session_files(&project_dir) else {
                    continue;
                };
                file_count += session_files.len();
                for session_file in session_files {
                    if let Ok(entries) = parser::parse_jsonl_file(&session_file) {
                        total_entries += entries.len();
                        all_entries.extend(parser::filter_entries_by_date(entries, ctx.date));
                    }
                }
            }
        }

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

        let sessions = build_sessions(&entries)?;
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
            ],
        })
    }
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
fn build_sessions(entries: &[LogEntry]) -> Result<Vec<CollectedSession>, parser::ParseError> {
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let tasks = analyzer::build_claude_session_tasks(entries, ClaudeAdapter::ID)?;

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
