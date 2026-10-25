//! Codex adapter: reads `<root>/YYYY/MM/DD/rollout-*.jsonl` session logs.
//! Subagent rollouts are not sessions; their task and result are spliced
//! into the interactive parent's prompt (SPEC.md, stage 4).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};

use super::subagent::{self, SubagentRun};
use super::{CollectContext, CollectedSession, SourceAdapter, SourceCollection, SourceFingerprint};
use crate::parser::codex::{self, CodexEntry};
use crate::{analyzer, config, parser};

/// Exclusion reason for sessions spawned by another Codex session.
pub const EXCLUDED_SUBAGENT: &str = "subagent";
/// Name used when a subagent rollout has no `agent_nickname`.
const SUBAGENT_NAME_DEFAULT: &str = "subagent";
/// Steps followed up `parent_thread_id` links. Measured chains are at most 2
/// deep (45 depth-1, 5 depth-2); the cap only guards against cycles.
const MAX_SUBAGENT_CHAIN: usize = 8;

#[derive(Debug, Default)]
pub struct CodexAdapter {
    /// Fixed roots for tests; `None` discovers roots from config, `CODEX_HOME`, and home.
    roots: Option<Vec<PathBuf>>,
}

impl CodexAdapter {
    pub const ID: &'static str = "codex";

    /// Reads only the given roots, skipping config overrides and home discovery.
    #[cfg(test)]
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots: Some(roots) }
    }

    fn resolve_roots(&self, config: Option<&config::Config>) -> Vec<PathBuf> {
        match &self.roots {
            Some(roots) => parser::roots::dedupe_existing_paths(roots.clone()),
            None => codex::discover_codex_session_roots(root_overrides(config)),
        }
    }
}

fn root_overrides(config: Option<&config::Config>) -> Option<&[String]> {
    config
        .and_then(|cfg| cfg.input.as_ref())
        .and_then(|input| input.codex_roots.as_deref())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SessionMergeKey {
    SessionId(String),
    RootAndFile {
        root: PathBuf,
        rollout_filename: String,
    },
}

impl SourceAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn collect(&self, ctx: &CollectContext) -> Result<SourceCollection, parser::ParseError> {
        let roots = self.resolve_roots(ctx.config);

        let mut merged_entries: HashMap<SessionMergeKey, Vec<CodexEntry>> = HashMap::new();
        for root in &roots {
            let Ok(session_files) = codex::list_session_files_for_local_date(root, ctx.date) else {
                continue;
            };

            for file in session_files {
                let Ok(entries) = codex::parse_codex_jsonl_file(&file) else {
                    continue;
                };
                let entries = codex::filter_entries_by_local_date(entries, ctx.date);
                let summary = codex::summarize_codex_entries(&entries);
                // Keep only sessions with actual conversation on the target date.
                if summary.user_count == 0 && summary.assistant_count == 0 {
                    continue;
                }

                let merge_key = if !summary.session_id.is_empty() {
                    SessionMergeKey::SessionId(summary.session_id)
                } else {
                    let rollout_filename = file
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| file.to_string_lossy().to_string());
                    SessionMergeKey::RootAndFile {
                        root: root.clone(),
                        rollout_filename,
                    }
                };

                merged_entries.entry(merge_key).or_default().extend(entries);
            }
        }

        let mut subagent_rollouts = Vec::new();
        let mut sessions = Vec::new();
        for (merge_key, mut entries) in merged_entries {
            entries = codex::dedupe_entries(entries);
            codex::sort_entries_by_timestamp(&mut entries);
            if codex::classify_codex_session(&entries) == codex::CodexSessionKind::Subagent {
                let own_id = match merge_key {
                    SessionMergeKey::SessionId(id) => Some(id),
                    SessionMergeKey::RootAndFile { .. } => None,
                };
                subagent_rollouts.push((own_id, entries));
                continue;
            }
            let summary = codex::summarize_codex_entries(&entries);
            if summary.user_count > 0 || summary.assistant_count > 0 {
                sessions.push((summary, entries));
            }
        }

        sessions.sort_by(|(_, left_entries), (_, right_entries)| {
            let left_first = left_entries.iter().filter_map(codex::entry_timestamp).min();
            let right_first = right_entries
                .iter()
                .filter_map(codex::entry_timestamp)
                .min();
            match (left_first, right_first) {
                (Some(left), Some(right)) => left.cmp(&right),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });

        let interactive_ids: HashSet<&str> = sessions
            .iter()
            .map(|(summary, _)| summary.session_id.as_str())
            .collect();
        let attachment = attach_subagents(&subagent_rollouts, &interactive_ids, ctx.date);

        let entry_count = sessions.iter().map(|(_, entries)| entries.len()).sum();
        let mut collected = Vec::with_capacity(sessions.len());
        for (summary, entries) in &sessions {
            let lines = attachment
                .runs_by_parent
                .get(&summary.session_id)
                .map(|runs| subagent::subagent_lines(runs))
                .unwrap_or_default();
            let task =
                analyzer::build_codex_session_task(entries, &summary.session_id, Self::ID, &lines)?;
            collected.push(CollectedSession {
                task,
                first_activity: activity_timestamps(entries).min(),
                last_activity: activity_timestamps(entries).max(),
                token_usage: None,
            });
        }

        Ok(SourceCollection {
            roots,
            fingerprint: SourceFingerprint {
                session_count: collected.len(),
                entry_count,
            },
            sessions: collected,
            excluded: vec![(EXCLUDED_SUBAGENT, subagent_rollouts.len())],
            verbose_lines: vec![crate::messages::verbose::subagents(
                self.display_name(),
                attachment.attached,
                attachment.unattached,
            )],
        })
    }
}

/// Subagent runs grouped by the interactive session they belong to.
struct SubagentAttachment {
    runs_by_parent: HashMap<String, Vec<SubagentRun>>,
    attached: usize,
    /// Runs whose `parent_thread_id` chain reaches no session of the day.
    unattached: usize,
}

/// What a subagent rollout's own `session_meta` says about its spawn.
struct SpawnInfo<'a> {
    parent_thread_id: Option<&'a str>,
    agent_path: Option<&'a str>,
    nickname: Option<&'a str>,
    started: DateTime<Utc>,
}

/// Links each subagent rollout to its interactive parent by following
/// `parent_thread_id` up through other subagents (depth 2 and beyond).
fn attach_subagents(
    rollouts: &[(Option<String>, Vec<CodexEntry>)],
    interactive_ids: &HashSet<&str>,
    date: NaiveDate,
) -> SubagentAttachment {
    let infos: Vec<Option<SpawnInfo>> = rollouts
        .iter()
        .map(|(own_id, entries)| own_id.as_deref().and_then(|id| own_spawn_info(entries, id)))
        .collect();
    let parent_of: HashMap<&str, &str> = rollouts
        .iter()
        .zip(&infos)
        .filter_map(|((own_id, _), info)| {
            Some((own_id.as_deref()?, info.as_ref()?.parent_thread_id?))
        })
        .collect();

    let mut runs_by_parent: HashMap<String, Vec<SubagentRun>> = HashMap::new();
    let mut unattached = 0;
    for ((_, entries), info) in rollouts.iter().zip(&infos) {
        let root = info
            .as_ref()
            .and_then(|info| info.parent_thread_id)
            .and_then(|parent| resolve_interactive_parent(parent, &parent_of, interactive_ids));
        let (Some(info), Some(root)) = (info, root) else {
            unattached += 1;
            continue;
        };
        let (task, result) = subagent_task_and_result(entries, info.agent_path);
        let at = entries
            .iter()
            .filter(|entry| !matches!(entry, CodexEntry::SessionMeta { .. }))
            .filter_map(codex::entry_timestamp)
            .min()
            .unwrap_or(info.started);
        runs_by_parent
            .entry(root.to_string())
            .or_default()
            .push(SubagentRun {
                name: info.nickname.unwrap_or(SUBAGENT_NAME_DEFAULT).to_string(),
                task,
                result,
                at,
            });
    }

    let mut attached = 0;
    runs_by_parent.retain(|_, runs| {
        *runs = subagent::keep_target_date(std::mem::take(runs), date);
        attached += runs.len();
        !runs.is_empty()
    });
    SubagentAttachment {
        runs_by_parent,
        attached,
        unattached,
    }
}

/// Reads the `session_meta` line of the rollout itself. Forked rollouts also
/// carry a copy of the parent's `session_meta`, which is skipped here.
fn own_spawn_info<'a>(entries: &'a [CodexEntry], own_id: &str) -> Option<SpawnInfo<'a>> {
    entries.iter().find_map(|entry| match entry {
        CodexEntry::SessionMeta {
            timestamp,
            session_id,
            agent_nickname,
            parent_thread_id,
            agent_path,
            ..
        } if session_id == own_id => Some(SpawnInfo {
            parent_thread_id: parent_thread_id.as_deref(),
            agent_path: agent_path.as_deref(),
            nickname: agent_nickname.as_deref(),
            started: *timestamp,
        }),
        _ => None,
    })
}

fn resolve_interactive_parent<'a>(
    mut current: &'a str,
    parent_of: &HashMap<&'a str, &'a str>,
    interactive_ids: &HashSet<&str>,
) -> Option<&'a str> {
    for _ in 0..MAX_SUBAGENT_CHAIN {
        if interactive_ids.contains(current) {
            return Some(current);
        }
        current = parent_of.get(current)?;
    }
    None
}

/// Task = first `agent_message` sent to this agent; result = last answer
/// after it. Forked rollouts start with a copy of the parent's history, so
/// nothing before that message belongs to the subagent. Rollouts without
/// such a message (older format, no copied history) use the first user text.
fn subagent_task_and_result(entries: &[CodexEntry], agent_path: Option<&str>) -> (String, String) {
    let to_me = agent_path.and_then(|path| {
        entries
            .iter()
            .enumerate()
            .find_map(|(index, entry)| match entry {
                CodexEntry::AgentMessage {
                    recipient, text, ..
                } if recipient == path => Some((index, text.clone())),
                _ => None,
            })
    });
    let (task, own_entries) = match to_me {
        Some((index, text)) => (text, &entries[index + 1..]),
        None => {
            let first_user = entries.iter().find_map(|entry| match entry {
                CodexEntry::UserMessage { text, .. } if !text.trim().is_empty() => {
                    Some(text.clone())
                }
                _ => None,
            });
            (first_user.unwrap_or_default(), entries)
        }
    };
    let result = own_entries
        .iter()
        .rev()
        .find_map(|entry| match entry {
            CodexEntry::AssistantMessage { output_text, .. } if !output_text.trim().is_empty() => {
                Some(output_text.clone())
            }
            _ => None,
        })
        .unwrap_or_default();
    (task, result)
}

/// Returns timestamps of conversation activity, skipping `SessionMeta`.
///
/// Session metadata can carry the session start time from a previous day,
/// so it must not stretch the displayed activity range.
fn activity_timestamps(entries: &[CodexEntry]) -> impl Iterator<Item = DateTime<Utc>> + '_ {
    entries.iter().filter_map(|entry| match entry {
        CodexEntry::UserMessage { timestamp, .. }
        | CodexEntry::AssistantMessage { timestamp, .. }
        | CodexEntry::FunctionCall { timestamp, .. } => Some(*timestamp),
        CodexEntry::SessionMeta { .. }
        | CodexEntry::AgentMessage { .. }
        | CodexEntry::Other { .. } => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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

    fn collect(roots: Vec<PathBuf>) -> SourceCollection {
        CodexAdapter::with_roots(roots)
            .collect(&CollectContext {
                date: date(),
                config: None,
                include_automated: false,
            })
            .expect("collect")
    }

    fn write_codex_rollout(file: &std::path::Path, session_id: &str, cwd: &str, extra_meta: &str) {
        let mut handle = std::fs::File::create(file).expect("rollout file");
        writeln!(
            handle,
            r#"{{"timestamp":"2099-01-01T12:00:00Z","type":"session_meta","payload":{{"id":"{session_id}","cwd":"{cwd}","model_provider":"openai"{extra_meta}}}}}"#
        )
        .expect("session meta");
        writeln!(
            handle,
            r#"{{"timestamp":"2099-01-01T12:01:00Z","type":"event_msg","payload":{{"type":"user_message","message":"hello"}}}}"#
        )
        .expect("user message");
        writeln!(
            handle,
            r#"{{"timestamp":"2099-01-01T12:02:00Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"done"}}]}}}}"#
        )
        .expect("assistant message");
    }

    #[test]
    fn test_collect_merges_sessions_by_id_across_roots() {
        let base = unique_temp_dir("rwd_test_sources_codex_multi_root");
        let root_a = base.join("codex-a");
        let root_b = base.join("codex-b");
        let day_dir_a = root_a.join("2099").join("01").join("01");
        let day_dir_b = root_b.join("2099").join("01").join("01");
        std::fs::create_dir_all(&day_dir_a).expect("day a");
        std::fs::create_dir_all(&day_dir_b).expect("day b");

        let mut file_a = std::fs::File::create(day_dir_a.join("rollout-a.jsonl")).expect("file a");
        writeln!(
            file_a,
            r#"{{"timestamp":"2099-01-01T12:00:00Z","type":"session_meta","payload":{{"id":"codex-s1","cwd":"/p","model_provider":"openai"}}}}"#
        )
        .expect("meta a");
        writeln!(
            file_a,
            r#"{{"timestamp":"2099-01-01T12:01:00Z","type":"event_msg","payload":{{"type":"user_message","message":"hello"}}}}"#
        )
        .expect("user a");

        let mut file_b = std::fs::File::create(day_dir_b.join("rollout-b.jsonl")).expect("file b");
        writeln!(
            file_b,
            r#"{{"timestamp":"2099-01-01T12:00:00Z","type":"session_meta","payload":{{"id":"codex-s1","cwd":"/p","model_provider":"openai"}}}}"#
        )
        .expect("meta b");
        writeln!(
            file_b,
            r#"{{"timestamp":"2099-01-01T12:01:00Z","type":"event_msg","payload":{{"type":"user_message","message":"hello"}}}}"#
        )
        .expect("user duplicate");
        writeln!(
            file_b,
            r#"{{"timestamp":"2099-01-01T12:02:00Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"done"}}]}}}}"#
        )
        .expect("assistant b");

        let collection = collect(vec![root_a.clone(), root_b.clone()]);
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.roots, vec![root_a, root_b]);
        assert_eq!(collection.sessions.len(), 1);
        let task = &collection.sessions[0].task;
        assert_eq!(task.session_id, "codex-s1");
        let user_count = task.messages.iter().filter(|(r, _)| r == "USER").count();
        let assistant_count = task
            .messages
            .iter()
            .filter(|(r, _)| r == "ASSISTANT")
            .count();
        assert_eq!(user_count, 1);
        assert_eq!(assistant_count, 1);
    }

    #[test]
    fn test_collect_excludes_explicit_subagent_sessions() {
        let base = unique_temp_dir("rwd_test_sources_codex_subagent_excluded");
        let root = base.join("codex-root");
        let day_dir = root.join("2099").join("01").join("01");
        std::fs::create_dir_all(&day_dir).expect("day dir");
        write_codex_rollout(
            &day_dir.join("rollout-subagent.jsonl"),
            "codex-subagent-1",
            "/Users/jinwoohan/.codex/memories",
            r#","source":{"subagent":"memory_consolidation"},"agent_role":"memory builder","agent_nickname":"Morpheus""#,
        );

        let collection = collect(vec![root.clone()]);
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.roots, vec![root]);
        assert!(collection.sessions.is_empty());
        assert_eq!(collection.excluded, vec![(EXCLUDED_SUBAGENT, 1)]);
        assert_eq!(collection.fingerprint, SourceFingerprint::default());
    }

    #[test]
    fn test_collect_excludes_mixed_session_id_when_merged_session_is_subagent() {
        let base = unique_temp_dir("rwd_test_sources_codex_mixed_session_id_subagent");
        let root_a = base.join("codex-a");
        let root_b = base.join("codex-b");
        let day_dir_a = root_a.join("2099").join("01").join("01");
        let day_dir_b = root_b.join("2099").join("01").join("01");
        std::fs::create_dir_all(&day_dir_a).expect("day a");
        std::fs::create_dir_all(&day_dir_b).expect("day b");

        let mut file_a = std::fs::File::create(day_dir_a.join("rollout-a.jsonl")).expect("file a");
        writeln!(
            file_a,
            r#"{{"timestamp":"2099-01-01T12:00:00Z","type":"session_meta","payload":{{"id":"codex-mixed-1","cwd":"/Users/jinwoohan/.codex/memories","model_provider":"openai","source":{{"subagent":"memory_consolidation"}},"agent_role":"memory builder","agent_nickname":"Morpheus"}}}}"#
        )
        .expect("subagent meta");
        writeln!(
            file_a,
            r#"{{"timestamp":"2099-01-01T12:01:00Z","type":"event_msg","payload":{{"type":"user_message","message":"hello from subagent"}}}}"#
        )
        .expect("subagent user");

        let mut file_b = std::fs::File::create(day_dir_b.join("rollout-b.jsonl")).expect("file b");
        writeln!(
            file_b,
            r#"{{"timestamp":"2099-01-01T12:00:00Z","type":"session_meta","payload":{{"id":"codex-mixed-1","cwd":"/Users/jinwoohan/workspace/repos/personal/rwd","model_provider":"openai","source":"cli"}}}}"#
        )
        .expect("interactive meta");
        writeln!(
            file_b,
            r#"{{"timestamp":"2099-01-01T12:01:00Z","type":"event_msg","payload":{{"type":"user_message","message":"hello from interactive"}}}}"#
        )
        .expect("interactive user");
        writeln!(
            file_b,
            r#"{{"timestamp":"2099-01-01T12:02:00Z","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"done"}}]}}}}"#
        )
        .expect("interactive assistant");

        let collection = collect(vec![root_a.clone(), root_b.clone()]);
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.roots, vec![root_a, root_b]);
        assert!(collection.sessions.is_empty());
    }

    #[test]
    fn test_collect_keeps_worktree_interactive_sessions_without_hard_signal() {
        let base = unique_temp_dir("rwd_test_sources_codex_worktree_interactive");
        let root = base.join("codex-root");
        let day_dir = root.join("2099").join("01").join("01");
        std::fs::create_dir_all(&day_dir).expect("day dir");

        let worktree_cwd = "/Users/jinwoohan/.codex/worktrees/1234/rwd";
        write_codex_rollout(
            &day_dir.join("rollout-interactive.jsonl"),
            "codex-worktree-1",
            worktree_cwd,
            r#","source":"cli""#,
        );

        let collection = collect(vec![root.clone()]);
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.roots, vec![root]);
        assert_eq!(collection.sessions.len(), 1);
        let task = &collection.sessions[0].task;
        assert_eq!(task.session_id, "codex-worktree-1");
        // The session metadata (with the worktree cwd) stays first in the prompt.
        assert_eq!(task.messages.first().map(|(r, _)| r.as_str()), Some("META"));
        assert!(task.messages[0].1.contains(worktree_cwd));
        assert_eq!(collection.excluded, vec![(EXCLUDED_SUBAGENT, 0)]);
    }

    #[test]
    fn test_collect_excludes_nested_thread_spawn_source() {
        let base = unique_temp_dir("rwd_test_sources_codex_nested_subagent");
        let root = base.join("codex-root");
        let day_dir = root.join("2099/01/01");
        std::fs::create_dir_all(&day_dir).expect("day dir");
        write_codex_rollout(
            &day_dir.join("rollout-nested-subagent.jsonl"),
            "nested-subagent",
            "/project",
            r#","source":{"subagent":{"thread_spawn":{"parent_thread_id":"019d0000-0000-7000-8000-000000000001","depth":1}}}"#,
        );

        let collection = collect(vec![root]);
        std::fs::remove_dir_all(base).expect("cleanup");

        assert!(
            collection.sessions.is_empty(),
            "nested subagent metadata must exclude the session without a role or nickname"
        );
    }

    #[test]
    fn test_collect_sample_builds_fixed_prompt_text() {
        let base = unique_temp_dir("rwd_test_sources_codex_fixed_prompt");
        let day_dir = base.join("2099/01/01");
        std::fs::create_dir_all(&day_dir).expect("day dir");
        write_codex_rollout(
            &day_dir.join("rollout-fixed.jsonl"),
            "codex-fixed",
            "/project",
            r#","source":"cli""#,
        );

        let collection = collect(vec![base.clone()]);
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.sessions.len(), 1);
        assert_eq!(
            collection.sessions[0].task.prompt_text,
            "[Session: codex-fixed]\n\
             [META] /project\ncodex-fixed\nopenai\ncli\n\
             [USER] hello\nhello\nuser_message\n\
             [ASSISTANT] done\ndone\noutput_text\nassistant\nmessage\n"
        );
        assert_eq!(
            collection.fingerprint,
            SourceFingerprint {
                session_count: 1,
                entry_count: 3,
            }
        );
    }

    fn subagent_meta(ts: &str, id: &str, parent: &str, path: &str, nickname: &str) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"session_meta","payload":{{"id":"{id}","forked_from_id":"{parent}","cwd":"/project","model_provider":"openai","history_mode":"paginated","source":{{"subagent":{{"thread_spawn":{{"parent_thread_id":"{parent}","depth":1,"agent_path":"{path}","agent_nickname":"{nickname}","agent_role":null}}}}}},"agent_nickname":"{nickname}","agent_path":"{path}"}}}}"#
        )
    }

    fn agent_message(ts: &str, author: &str, recipient: &str, text: &str) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"response_item","payload":{{"type":"agent_message","author":"{author}","recipient":"{recipient}","content":[{{"type":"input_text","text":"{text}"}}]}}}}"#
        )
    }

    fn assistant(ts: &str, text: &str) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"{text}"}}]}}}}"#
        )
    }

    /// Interactive parent `codex-parent` and its forked subagent `codex-child`
    /// (Ada), whose rollout starts with a copy of the parent's history.
    fn write_parent_and_forked_child(day_dir: &std::path::Path) {
        std::fs::create_dir_all(day_dir).expect("day dir");
        std::fs::write(
            day_dir.join("rollout-parent.jsonl"),
            [
                r#"{"timestamp":"2099-01-01T12:00:00Z","type":"session_meta","payload":{"id":"codex-parent","cwd":"/project","model_provider":"openai","source":"cli"}}"#.to_string(),
                r#"{"timestamp":"2099-01-01T12:01:00Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"PARENT_FIRST_QUESTION"}]}}"#.to_string(),
                r#"{"timestamp":"2099-01-01T12:02:00Z","type":"response_item","payload":{"type":"function_call","name":"spawn_agent","arguments":"{\"message\":\"SPAWN_ARG_MARKER\"}"}}"#.to_string(),
                agent_message("2099-01-01T12:10:00Z", "/root/ada", "/root", "CHILD_REPORT_COPY"),
                assistant("2099-01-01T12:11:00Z", "Thanks, merged."),
            ]
            .join("\n"),
        )
        .expect("parent rollout");
        std::fs::write(
            day_dir.join("rollout-child.jsonl"),
            [
                subagent_meta("2099-01-01T12:02:00.100Z", "codex-child", "codex-parent", "/root/ada", "Ada"),
                r#"{"timestamp":"2099-01-01T12:02:00.200Z","type":"session_meta","payload":{"id":"codex-parent","cwd":"/project","model_provider":"openai","source":"cli"}}"#.to_string(),
                r#"{"timestamp":"2099-01-01T12:02:00.300Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"PARENT_FIRST_QUESTION"}]}}"#.to_string(),
                assistant("2099-01-01T12:02:00.400Z", "COPIED_PARENT_ANSWER"),
                agent_message("2099-01-01T12:02:01Z", "/root", "/root/ada", "Check parser edge cases"),
                assistant("2099-01-01T12:03:00Z", "SUBAGENT_MIDDLE_TEXT"),
                r#"{"timestamp":"2099-01-01T12:04:00Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"TOOL_ARG_MARKER\"}"}}"#.to_string(),
                assistant("2099-01-01T12:09:00Z", "Found two edge cases"),
            ]
            .join("\n"),
        )
        .expect("child rollout");
    }

    #[test]
    fn test_collect_attaches_forked_subagent_task_and_result_to_parent() {
        let base = unique_temp_dir("rwd_test_sources_codex_subagent_attach");
        write_parent_and_forked_child(&base.join("2099/01/01"));

        let collection = collect(vec![base.clone()]);
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.sessions.len(), 1);
        let task = &collection.sessions[0].task;
        assert_eq!(task.session_id, "codex-parent");
        let prompt = &task.prompt_text;
        assert!(
            prompt.contains(
                "[TOOL] spawn_agent\n\
                 [SUBAGENT] Ada: Check parser edge cases\n\
                 [SUBAGENT_RESULT] Found two edge cases\n\
                 [EVENT:response_item]\n"
            ),
            "{prompt}"
        );
        for marker in [
            "COPIED_PARENT_ANSWER",
            "SUBAGENT_MIDDLE_TEXT",
            "TOOL_ARG_MARKER",
            "SPAWN_ARG_MARKER",
            "CHILD_REPORT_COPY",
        ] {
            assert!(!prompt.contains(marker), "{marker} leaked:\n{prompt}");
        }
        assert_eq!(prompt.matches("PARENT_FIRST_QUESTION").count(), 1);
    }

    #[test]
    fn test_collect_subagents_stay_out_of_codex_session_count_and_fingerprint() {
        let base = unique_temp_dir("rwd_test_sources_codex_subagent_count");
        write_parent_and_forked_child(&base.join("2099/01/01"));

        let collection = collect(vec![base.clone()]);
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(
            collection.fingerprint,
            SourceFingerprint {
                session_count: 1,
                entry_count: 5,
            }
        );
        assert_eq!(collection.excluded, vec![(EXCLUDED_SUBAGENT, 1)]);
        assert_eq!(
            collection.verbose_lines,
            vec!["[discover] Codex subagents: 1 attached".to_string()]
        );
    }

    #[test]
    fn test_collect_attaches_nested_subagent_to_interactive_parent() {
        let base = unique_temp_dir("rwd_test_sources_codex_subagent_nested");
        let day_dir = base.join("2099/01/01");
        write_parent_and_forked_child(&day_dir);
        std::fs::write(
            day_dir.join("rollout-grandchild.jsonl"),
            [
                subagent_meta(
                    "2099-01-01T12:05:00Z",
                    "codex-grandchild",
                    "codex-child",
                    "/root/ada/bo",
                    "Bo",
                ),
                agent_message(
                    "2099-01-01T12:05:01Z",
                    "/root/ada",
                    "/root/ada/bo",
                    "Dig deeper",
                ),
                assistant("2099-01-01T12:06:00Z", "Deep result"),
            ]
            .join("\n"),
        )
        .expect("grandchild rollout");

        let collection = collect(vec![base.clone()]);
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.sessions.len(), 1);
        let prompt = &collection.sessions[0].task.prompt_text;
        assert!(
            prompt.contains(
                "[SUBAGENT_RESULT] Found two edge cases\n\
                 [SUBAGENT] Bo: Dig deeper\n\
                 [SUBAGENT_RESULT] Deep result\n\
                 [EVENT:response_item]\n"
            ),
            "{prompt}"
        );
        assert_eq!(collection.excluded, vec![(EXCLUDED_SUBAGENT, 2)]);
    }

    #[test]
    fn test_collect_counts_subagent_without_parent_session_in_verbose_only() {
        let base = unique_temp_dir("rwd_test_sources_codex_subagent_orphan");
        let day_dir = base.join("2099/01/01");
        write_parent_and_forked_child(&day_dir);
        std::fs::write(
            day_dir.join("rollout-orphan.jsonl"),
            [
                subagent_meta(
                    "2099-01-01T13:00:00Z",
                    "codex-orphan",
                    "missing-parent",
                    "/root/zed",
                    "Zed",
                ),
                agent_message("2099-01-01T13:00:01Z", "/root", "/root/zed", "ORPHAN_TASK"),
                assistant("2099-01-01T13:01:00Z", "ORPHAN_RESULT"),
            ]
            .join("\n"),
        )
        .expect("orphan rollout");

        let collection = collect(vec![base.clone()]);
        std::fs::remove_dir_all(&base).ok();

        let prompt = &collection.sessions[0].task.prompt_text;
        assert!(!prompt.contains("ORPHAN_TASK"));
        assert!(!prompt.contains("ORPHAN_RESULT"));
        assert_eq!(
            collection.verbose_lines,
            vec![
                "[discover] Codex subagents: 1 attached, 1 not attached (no parent session found)"
                    .to_string()
            ]
        );
    }

    #[test]
    fn test_collect_subagent_without_agent_message_uses_first_user_text() {
        let base = unique_temp_dir("rwd_test_sources_codex_subagent_legacy");
        let day_dir = base.join("2099/01/01");
        write_parent_and_forked_child(&day_dir);
        std::fs::write(
            day_dir.join("rollout-legacy.jsonl"),
            [
                subagent_meta("2099-01-01T12:20:00Z", "codex-legacy", "codex-parent", "/root/rex", "Rex"),
                r##"{"timestamp":"2099-01-01T12:20:01Z","type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>ctx</environment_context>"},{"type":"input_text","text":"Legacy task"}]}}"##.to_string(),
                assistant("2099-01-01T12:21:00Z", "Legacy result"),
            ]
            .join("\n"),
        )
        .expect("legacy rollout");

        let collection = collect(vec![base.clone()]);
        std::fs::remove_dir_all(&base).ok();

        let prompt = &collection.sessions[0].task.prompt_text;
        assert!(
            prompt.contains("[SUBAGENT] Rex: Legacy task\n[SUBAGENT_RESULT] Legacy result\n"),
            "{prompt}"
        );
    }

    #[test]
    fn test_collect_activity_range_ignores_previous_day_session_meta() {
        use chrono::TimeZone;

        let local = |day: u32, hour: u32, minute: u32| {
            chrono::Local
                .with_ymd_and_hms(2099, 1, day, hour, minute, 0)
                .earliest()
                .expect("local time")
                .with_timezone(&Utc)
                .to_rfc3339()
        };
        let base = unique_temp_dir("rwd_test_sources_codex_activity");
        let target = chrono::NaiveDate::from_ymd_opt(2099, 1, 2).expect("date");
        // Write the file under every UTC day the local target day touches.
        let utc_days = crate::parser::utc_dates_for_local_date(target).expect("utc days");
        let day_dir = base.join(utc_days[0].format("%Y/%m/%d").to_string());
        std::fs::create_dir_all(&day_dir).expect("day dir");
        std::fs::write(
            day_dir.join("rollout-activity.jsonl"),
            [
                format!(
                    r#"{{"timestamp":"{}","type":"session_meta","payload":{{"id":"s1","cwd":"/tmp","model_provider":"openai"}}}}"#,
                    local(1, 23, 28)
                ),
                format!(
                    r#"{{"timestamp":"{}","type":"event_msg","payload":{{"type":"user_message","message":"hello"}}}}"#,
                    local(2, 0, 10)
                ),
                format!(
                    r#"{{"timestamp":"{}","type":"response_item","payload":{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":"done"}}]}}}}"#,
                    local(2, 20, 44)
                ),
            ]
            .join("\n"),
        )
        .expect("write rollout");

        let collection = CodexAdapter::with_roots(vec![base.clone()])
            .collect(&CollectContext {
                date: target,
                config: None,
                include_automated: false,
            })
            .expect("collect");
        std::fs::remove_dir_all(&base).ok();

        assert_eq!(collection.sessions.len(), 1);
        let session = &collection.sessions[0];
        let to_local = |ts: Option<DateTime<Utc>>| {
            ts.expect("activity")
                .with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        };
        assert_eq!(to_local(session.first_activity), "00:10");
        assert_eq!(to_local(session.last_activity), "20:44");
        assert_eq!(session.token_usage, None);
    }
}
