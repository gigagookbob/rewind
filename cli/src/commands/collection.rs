//! Discover and collect session logs without changing source-specific filtering.

use crate::{config, parser};

pub(super) type CodexCollectedSession = (
    parser::codex::CodexSessionSummary,
    Vec<parser::codex::CodexEntry>,
);

/// Stats from log discovery (for verbose output).
pub(super) struct DiscoveryStats {
    pub(super) project_count: usize,
    pub(super) file_count: usize,
    pub(super) total_entries: usize,
    pub(super) claude_interactive_sessions: usize,
    pub(super) claude_automated_sessions: usize,
    pub(super) roots: Vec<std::path::PathBuf>,
}

fn claude_root_overrides(config: Option<&config::Config>) -> Option<&[String]> {
    config.and_then(|cfg| cfg.input.as_ref()).and_then(|input| {
        input
            .claude
            .as_ref()
            .and_then(|claude| claude.roots.as_deref())
            .or(input.claude_roots.as_deref())
    })
}

pub(super) fn claude_include_automated(config: Option<&config::Config>) -> bool {
    config
        .and_then(|cfg| cfg.input.as_ref())
        .and_then(|input| input.claude.as_ref())
        .map(|claude| claude.include_automated)
        .unwrap_or(false)
}

fn codex_root_overrides(config: Option<&config::Config>) -> Option<&[String]> {
    config
        .and_then(|cfg| cfg.input.as_ref())
        .and_then(|input| input.codex_roots.as_deref())
}

/// Collects Claude Code log entries with discovery statistics.
pub(super) fn collect_claude_entries_with_stats(
    today: chrono::NaiveDate,
    config: Option<&config::Config>,
    include_automated: bool,
) -> (Vec<parser::claude::LogEntry>, DiscoveryStats) {
    let roots = parser::discover_claude_log_roots(claude_root_overrides(config));
    let mut stats = DiscoveryStats {
        project_count: 0,
        file_count: 0,
        total_entries: 0,
        claude_interactive_sessions: 0,
        claude_automated_sessions: 0,
        roots: roots.clone(),
    };

    if roots.is_empty() {
        return (Vec::new(), stats);
    }

    let mut all_entries = Vec::new();
    for root in roots {
        if let Ok(project_dirs) = parser::list_project_dirs_in_root(&root) {
            stats.project_count += project_dirs.len();
            for project_dir in project_dirs {
                if let Ok(session_files) = parser::list_session_files(&project_dir) {
                    stats.file_count += session_files.len();
                    for session_file in session_files {
                        if let Ok(entries) = parser::parse_jsonl_file(&session_file) {
                            stats.total_entries += entries.len();
                            let today_entries = parser::filter_entries_by_date(entries, today);
                            all_entries.extend(today_entries);
                        }
                    }
                }
            }
        }
    }
    let deduped = parser::dedupe_claude_entries(all_entries);
    let filtered = parser::filter_automated_sessions(deduped, include_automated);
    stats.claude_interactive_sessions = filtered.counts.interactive;
    stats.claude_automated_sessions = filtered.counts.automated;
    let mut deduped = filtered.entries;
    deduped.sort_by(|left, right| {
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

    (deduped, stats)
}

/// Collects Claude Code log entries. Returns empty Vec if the log directory is missing.
pub(super) fn collect_claude_entries(
    today: chrono::NaiveDate,
    config: Option<&config::Config>,
    include_automated: bool,
) -> Vec<parser::claude::LogEntry> {
    collect_claude_entries_with_stats(today, config, include_automated).0
}

/// Collects Codex session logs. Returns empty Vec if the sessions directory is missing.
pub(super) fn collect_codex_sessions(
    today: chrono::NaiveDate,
    config: Option<&config::Config>,
) -> (Vec<CodexCollectedSession>, Vec<std::path::PathBuf>) {
    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    enum SessionMergeKey {
        SessionId(String),
        RootAndFile {
            root: std::path::PathBuf,
            rollout_filename: String,
        },
    }

    let roots = parser::codex::discover_codex_session_roots(codex_root_overrides(config));
    if roots.is_empty() {
        return (Vec::new(), roots);
    }

    let mut merged_entries: std::collections::HashMap<
        SessionMergeKey,
        Vec<parser::codex::CodexEntry>,
    > = std::collections::HashMap::new();

    for root in &roots {
        let Ok(session_files) = parser::codex::list_session_files_for_local_date(root, today)
        else {
            continue;
        };

        for file in session_files {
            let Ok(entries) = parser::codex::parse_codex_jsonl_file(&file) else {
                continue;
            };
            let entries = parser::codex::filter_entries_by_local_date(entries, today);
            let summary = parser::codex::summarize_codex_entries(&entries);
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

    let mut sessions = Vec::new();
    for mut entries in merged_entries.into_values() {
        entries = parser::codex::dedupe_entries(entries);
        parser::codex::sort_entries_by_timestamp(&mut entries);
        if parser::codex::classify_codex_session(&entries)
            == parser::codex::CodexSessionKind::Subagent
        {
            continue;
        }
        let summary = parser::codex::summarize_codex_entries(&entries);
        if summary.user_count > 0 || summary.assistant_count > 0 {
            sessions.push((summary, entries));
        }
    }

    sessions.sort_by(|(_, left_entries), (_, right_entries)| {
        let left_first = left_entries
            .iter()
            .filter_map(parser::codex::entry_timestamp)
            .min();
        let right_first = right_entries
            .iter()
            .filter_map(parser::codex::entry_timestamp)
            .min();
        match (left_first, right_first) {
            (Some(left), Some(right)) => left.cmp(&right),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });

    (sessions, roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_config(
        codex_roots: Option<Vec<String>>,
        claude_roots: Option<Vec<String>>,
    ) -> config::Config {
        config::Config {
            llm: config::LlmConfig {
                provider: "codex".to_string(),
                openai_api_key: String::new(),
                anthropic_api_key: String::new(),
                legacy_api_key: String::new(),
                codex_model: None,
                codex_reasoning_effort: None,
            },
            output: config::OutputConfig {
                path: "/tmp".to_string(),
            },
            redactor: None,
            lang: Some(config::Lang::En),
            input: Some(config::InputConfig {
                codex_roots,
                claude_roots,
                claude: None,
            }),
        }
    }

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ))
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
    fn test_collect_claude_entries_dedupes_across_roots() {
        // Use a far-future date to avoid collisions with real local logs.
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date");
        let base = unique_temp_dir("rwd_test_claude_multi_root");
        let root_a = base.join("claude-a");
        let root_b = base.join("claude-b");
        let project_a = root_a.join("project-a");
        let project_b = root_b.join("project-b");
        std::fs::create_dir_all(&project_a).expect("project a");
        std::fs::create_dir_all(&project_b).expect("project b");

        let line = r#"{"type":"user","sessionId":"claude-session-1","timestamp":"2099-01-01T12:00:00Z","uuid":"same-entry"}"#;
        let file_a = project_a.join("session-a.jsonl");
        let file_b = project_b.join("session-b.jsonl");
        std::fs::write(&file_a, format!("{line}\n")).expect("write a");
        std::fs::write(&file_b, format!("{line}\n")).expect("write b");

        let cfg = test_config(
            None,
            Some(vec![
                root_a.to_string_lossy().to_string(),
                root_b.to_string_lossy().to_string(),
            ]),
        );
        let (entries, stats) = collect_claude_entries_with_stats(date, Some(&cfg), false);

        assert!(stats.roots.starts_with(&[root_a.clone(), root_b.clone()]));
        let matching_count = entries
            .iter()
            .filter(|entry| {
                matches!(
                    entry,
                    parser::claude::LogEntry::User(u)
                        if u.session_id == "claude-session-1" && u.uuid == "same-entry"
                )
            })
            .count();
        assert_eq!(matching_count, 1);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_collect_codex_sessions_merges_by_session_id_across_roots() {
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date");
        let base = unique_temp_dir("rwd_test_codex_multi_root");
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

        let cfg = test_config(
            Some(vec![
                root_a.to_string_lossy().to_string(),
                root_b.to_string_lossy().to_string(),
            ]),
            None,
        );
        let (sessions, roots) = collect_codex_sessions(date, Some(&cfg));

        assert!(roots.starts_with(&[root_a.clone(), root_b.clone()]));
        assert_eq!(sessions.len(), 1);
        let (summary, _) = &sessions[0];
        assert_eq!(summary.session_id, "codex-s1");
        assert_eq!(summary.user_count, 1);
        assert_eq!(summary.assistant_count, 1);

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_collect_codex_sessions_excludes_explicit_subagent_sessions() {
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date");
        let base = unique_temp_dir("rwd_test_codex_subagent_excluded");
        let root = base.join("codex-root");
        let day_dir = root.join("2099").join("01").join("01");
        std::fs::create_dir_all(&day_dir).expect("day dir");

        let rollout_file = day_dir.join("rollout-subagent.jsonl");
        write_codex_rollout(
            &rollout_file,
            "codex-subagent-1",
            "/Users/jinwoohan/.codex/memories",
            r#","source":{"subagent":"memory_consolidation"},"agent_role":"memory builder","agent_nickname":"Morpheus""#,
        );

        let cfg = test_config(Some(vec![root.to_string_lossy().to_string()]), None);
        let (sessions, roots) = collect_codex_sessions(date, Some(&cfg));

        assert!(roots.starts_with(std::slice::from_ref(&root)));
        assert!(sessions.is_empty());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_collect_codex_sessions_excludes_mixed_session_id_when_merged_session_is_subagent() {
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date");
        let base = unique_temp_dir("rwd_test_codex_mixed_session_id_subagent");
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

        let cfg = test_config(
            Some(vec![
                root_a.to_string_lossy().to_string(),
                root_b.to_string_lossy().to_string(),
            ]),
            None,
        );
        let (sessions, roots) = collect_codex_sessions(date, Some(&cfg));

        assert!(roots.starts_with(&[root_a.clone(), root_b.clone()]));
        assert!(sessions.is_empty());

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_collect_codex_sessions_keeps_worktree_interactive_sessions_without_hard_signal() {
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date");
        let base = unique_temp_dir("rwd_test_codex_worktree_interactive");
        let root = base.join("codex-root");
        let day_dir = root.join("2099").join("01").join("01");
        std::fs::create_dir_all(&day_dir).expect("day dir");

        let worktree_cwd = "/Users/jinwoohan/.codex/worktrees/1234/rwd".to_string();
        let rollout_file = day_dir.join("rollout-interactive.jsonl");
        write_codex_rollout(
            &rollout_file,
            "codex-worktree-1",
            &worktree_cwd,
            r#","source":"cli""#,
        );

        let cfg = test_config(Some(vec![root.to_string_lossy().to_string()]), None);
        let (sessions, roots) = collect_codex_sessions(date, Some(&cfg));

        assert!(roots.starts_with(std::slice::from_ref(&root)));
        assert_eq!(sessions.len(), 1);
        let (summary, entries) = &sessions[0];
        assert_eq!(summary.session_id, "codex-worktree-1");
        assert_eq!(summary.cwd, worktree_cwd);
        assert_eq!(
            parser::codex::classify_codex_session(entries),
            parser::codex::CodexSessionKind::Interactive
        );
        assert!(matches!(
            entries.first(),
            Some(parser::codex::CodexEntry::SessionMeta { .. })
        ));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn test_collect_codex_sessions_excludes_nested_thread_spawn_source() {
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date");
        let base = unique_temp_dir("rwd_test_codex_nested_subagent");
        let root = base.join("codex-root");
        let day_dir = root.join("2099/01/01");
        std::fs::create_dir_all(&day_dir).expect("day dir");
        write_codex_rollout(
            &day_dir.join("rollout-nested-subagent.jsonl"),
            "nested-subagent",
            "/project",
            r#","source":{"subagent":{"thread_spawn":{"parent_thread_id":"019d0000-0000-7000-8000-000000000001","depth":1}}}"#,
        );
        let cfg = test_config(Some(vec![root.to_string_lossy().to_string()]), None);
        let (sessions, _) = collect_codex_sessions(date, Some(&cfg));
        std::fs::remove_dir_all(base).expect("cleanup");
        assert!(
            sessions.is_empty(),
            "nested subagent metadata must exclude the session without a role or nickname"
        );
    }
}
