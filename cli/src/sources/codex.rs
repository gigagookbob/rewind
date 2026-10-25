//! Codex adapter: reads `<root>/YYYY/MM/DD/rollout-*.jsonl` session logs.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};

use super::{CollectContext, CollectedSession, SourceAdapter, SourceCollection, SourceFingerprint};
use crate::parser::codex::{self, CodexEntry};
use crate::{analyzer, config, parser};

/// Exclusion reason for sessions spawned by another Codex session.
pub const EXCLUDED_SUBAGENT: &str = "subagent";

#[derive(Debug, Default)]
pub struct CodexAdapter {
    /// Fixed roots for tests; `None` discovers roots from config, `CODEX_HOME`, and home.
    roots: Option<Vec<PathBuf>>,
}

impl CodexAdapter {
    pub const ID: &'static str = "codex";

    /// Reads only the given roots, skipping config overrides and home discovery.
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

        let mut subagent_count = 0;
        let mut sessions = Vec::new();
        for mut entries in merged_entries.into_values() {
            entries = codex::dedupe_entries(entries);
            codex::sort_entries_by_timestamp(&mut entries);
            if codex::classify_codex_session(&entries) == codex::CodexSessionKind::Subagent {
                subagent_count += 1;
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

        let entry_count = sessions.iter().map(|(_, entries)| entries.len()).sum();
        let mut collected = Vec::with_capacity(sessions.len());
        for (summary, entries) in &sessions {
            let task = analyzer::build_codex_session_task(entries, &summary.session_id)?;
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
            excluded: vec![(EXCLUDED_SUBAGENT, subagent_count)],
            verbose_lines: Vec::new(),
        })
    }
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
        CodexEntry::SessionMeta { .. } | CodexEntry::Other { .. } => None,
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
