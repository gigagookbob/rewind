//! Antigravity adapter: reads agy CLI, Antigravity app, and Antigravity IDE
//! conversations from `<brain>/<conversation id>/.system_generated/logs/transcript_full.jsonl`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::{CollectContext, CollectedSession, SourceAdapter, SourceCollection, SourceFingerprint};
use crate::analyzer::{self, compaction::compact_log_like};
use crate::config::{self, AntigravityInputConfig};
use crate::messages::verbose;
use crate::parser::antigravity::{self, STEP_PLANNER_RESPONSE, STEP_USER_INPUT, TranscriptStep};
use crate::parser::{self, roots};

/// Exclusion reason for non-interactive `agy -p` runs.
pub const EXCLUDED_PRINT_MODE: &str = "print-mode";

/// agy's data folder under `~/.gemini`. Only conversations under it get the
/// `agy -p` rule (SPEC.md, skipping -p runs).
const AGY_DATA_DIR: &str = "antigravity-cli";
/// Data folders under `~/.gemini` in discovery order: agy, the Antigravity
/// app, the Antigravity IDE. Which app writes which folder is guessed from the
/// names; both app folders are read the same way (SPEC.md, files read).
const GEMINI_DATA_DIRS: [&str; 3] = [AGY_DATA_DIR, "antigravity", "antigravity-ide"];
const BRAIN_DIR: &str = "brain";
/// agy prompt history next to `brain/`. Verified: it lists every interactive
/// agy conversation and none of the 21 non-interactive runs (SPEC.md, skipping -p runs).
const AGY_HISTORY_FILE: &str = "history.jsonl";
/// Full transcript inside a conversation folder. The sibling `transcript.jsonl`
/// (truncated copy) and `chunks/` (split copy) are never read (SPEC.md, files read).
const TRANSCRIPT_PATH: [&str; 3] = [".system_generated", "logs", "transcript_full.jsonl"];
/// Conversations with at least this many user inputs count as interactive
/// even when missing from history. Inferred: all 21 non-interactive runs had
/// exactly one input (SPEC.md, skipping -p runs).
const INTERACTIVE_MIN_USER_INPUTS: usize = 2;

#[derive(Debug, Default)]
pub struct AntigravityAdapter {
    /// Fixed brain roots for tests; `None` discovers roots from config and home.
    roots: Option<Vec<PathBuf>>,
}

impl AntigravityAdapter {
    pub const ID: &'static str = "antigravity";

    /// Reads only the given brain roots, skipping config overrides and home discovery.
    #[cfg(test)]
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots: Some(roots) }
    }

    fn resolve_roots(&self, settings: &AntigravityInputConfig) -> Vec<PathBuf> {
        match &self.roots {
            Some(roots) => roots::dedupe_existing_paths(roots.clone()),
            None => discover_brain_roots(settings.roots.as_deref()),
        }
    }
}

/// `[input.antigravity]`, or its defaults (enabled, `agy -p` runs skipped) when absent.
fn settings(config: Option<&config::Config>) -> AntigravityInputConfig {
    config
        .and_then(|cfg| cfg.input.as_ref())
        .and_then(|input| input.antigravity.clone())
        .unwrap_or_default()
}

/// Config roots first, then `~/.gemini/<data dir>/brain`, then the same
/// folders under each Windows home when running inside WSL.
fn discover_brain_roots(config_roots: Option<&[String]>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = config_roots
        .unwrap_or_default()
        .iter()
        .map(PathBuf::from)
        .collect();

    if let Some(home) = dirs::home_dir() {
        candidates.extend(brain_dirs_under_home(&home));
    }

    if cfg!(target_os = "linux") && roots::is_wsl_environment() {
        for win_home in roots::wsl_windows_home_candidates() {
            candidates.extend(brain_dirs_under_home(&win_home));
        }
    }

    roots::dedupe_existing_paths(candidates)
}

fn brain_dirs_under_home(home: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    GEMINI_DATA_DIRS
        .iter()
        .map(move |dir| home.join(".gemini").join(dir).join(BRAIN_DIR))
}

/// Returns agy's `history.jsonl` when `root` is an agy `brain` folder
/// (`<...>/antigravity-cli/brain`); `None` for app, IDE, and other folders.
fn agy_history_path(root: &Path) -> Option<PathBuf> {
    let data_dir = root.parent()?;
    (data_dir.file_name()? == AGY_DATA_DIR).then(|| data_dir.join(AGY_HISTORY_FILE))
}

/// One conversation id, possibly found under several brain roots.
#[derive(Debug, Default)]
struct Conversation {
    steps: Vec<TranscriptStep>,
    /// History files of the agy roots holding this conversation.
    agy_histories: Vec<PathBuf>,
    /// Found under a non-agy root (app, IDE, or another configured folder).
    in_app_root: bool,
}

#[derive(Debug, Default)]
struct ScanStats {
    found: usize,
    unmodified: usize,
    read: usize,
    /// `-v` lines for folders and files that could not be read.
    unreadable: Vec<String>,
}

/// Reads every transcript written on or after `window_start`, grouped by
/// conversation id in root order.
fn load_conversations(
    roots: &[PathBuf],
    window_start: DateTime<Utc>,
    stats: &mut ScanStats,
) -> BTreeMap<String, Conversation> {
    let mut conversations: BTreeMap<String, Conversation> = BTreeMap::new();
    for root in roots {
        let history = agy_history_path(root);
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(e) => {
                stats.unreadable.push(verbose::antigravity_unreadable(
                    &root.display(),
                    &e.to_string(),
                ));
                continue;
            }
        };

        for entry in entries.flatten() {
            let transcript = TRANSCRIPT_PATH
                .iter()
                .fold(entry.path(), |path, part| path.join(part));
            if !transcript.is_file() {
                continue;
            }
            stats.found += 1;
            if modified_before(&transcript, window_start) {
                stats.unmodified += 1;
                continue;
            }
            let steps = match antigravity::parse_transcript_file(&transcript) {
                Ok(steps) => steps,
                Err(e) => {
                    stats.unreadable.push(verbose::antigravity_unreadable(
                        &transcript.display(),
                        &e.to_string(),
                    ));
                    continue;
                }
            };
            stats.read += 1;

            let id = entry.file_name().to_string_lossy().into_owned();
            let conversation = conversations.entry(id).or_default();
            conversation.steps.extend(steps);
            match &history {
                Some(path) if !conversation.agy_histories.contains(path) => {
                    conversation.agy_histories.push(path.clone());
                }
                Some(_) => {}
                None => conversation.in_app_root = true,
            }
        }
    }
    conversations
}

/// True when the file was last written before `start`. Such a file cannot
/// hold steps from the target date, so it is not opened. The modification
/// time is never used as activity time. Unknown times count as recent.
fn modified_before(path: &Path, start: DateTime<Utc>) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map(|modified| DateTime::<Utc>::from(modified) < start)
        .unwrap_or(false)
}

/// Orders steps by `step_index` and drops repeats of the same index, which
/// appear when one conversation id exists under two roots.
fn merge_steps(steps: &mut Vec<TranscriptStep>) {
    steps.sort_by_key(|step| (step.step_index.is_none(), step.step_index));
    steps.dedup_by(|later, earlier| {
        later.step_index.is_some() && later.step_index == earlier.step_index
    });
}

/// Reads each agy history once. `None` entries mean the file could not be read.
fn load_histories(
    conversations: &BTreeMap<String, Conversation>,
    unreadable: &mut Vec<String>,
) -> HashMap<PathBuf, Option<HashSet<String>>> {
    let mut histories = HashMap::new();
    for path in conversations.values().flat_map(|c| &c.agy_histories) {
        if histories.contains_key(path) {
            continue;
        }
        let ids = antigravity::read_history_conversation_ids(path);
        if ids.is_none() {
            unreadable.push(verbose::antigravity_history_missing(&path.display()));
        }
        histories.insert(path.clone(), ids);
    }
    histories
}

/// `agy -p` heuristic (SPEC.md, skipping -p runs): a conversation found only
/// under agy roots, with fewer than two user inputs, whose id is missing from
/// every agy history. When a history cannot be read the run cannot be judged,
/// so the conversation is kept.
fn is_print_mode(
    id: &str,
    conversation: &Conversation,
    histories: &HashMap<PathBuf, Option<HashSet<String>>>,
) -> bool {
    if conversation.in_app_root || conversation.agy_histories.is_empty() {
        return false;
    }
    let user_inputs = conversation
        .steps
        .iter()
        .filter(|step| step.step_type == STEP_USER_INPUT)
        .count();
    if user_inputs >= INTERACTIVE_MIN_USER_INPUTS {
        return false;
    }
    conversation
        .agy_histories
        .iter()
        .all(|path| matches!(histories.get(path), Some(Some(ids)) if !ids.contains(id)))
}

fn is_conversation_step(step: &TranscriptStep) -> bool {
    step.step_type == STEP_USER_INPUT || step.step_type == STEP_PLANNER_RESPONSE
}

/// Builds `(role, text)` pairs in step order: `USER` for the `<USER_REQUEST>`
/// text, one `ASSISTANT` line per response (thinking, then answer), and one
/// `TOOL` line per tool name. Tool args and other step types are dropped.
fn conversation_messages(steps: &[&TranscriptStep]) -> Vec<(String, String)> {
    let mut messages = Vec::new();
    for step in steps {
        match step.step_type.as_str() {
            STEP_USER_INPUT => {
                let request =
                    antigravity::extract_user_request(step.content.as_deref().unwrap_or_default());
                if !request.is_empty() {
                    messages.push(("USER".to_string(), compact_log_like(request)));
                }
            }
            STEP_PLANNER_RESPONSE => {
                let parts: Vec<&str> = [step.thinking.as_deref(), step.content.as_deref()]
                    .into_iter()
                    .flatten()
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .collect();
                if !parts.is_empty() {
                    messages.push(("ASSISTANT".to_string(), compact_log_like(&parts.join("\n"))));
                }
                for call in &step.tool_calls {
                    let name = call.name.trim();
                    if !name.is_empty() {
                        messages.push(("TOOL".to_string(), name.to_string()));
                    }
                }
            }
            _ => {}
        }
    }
    messages
}

fn build_prompt(session_id: &str, messages: &[(String, String)]) -> String {
    let mut output = format!("[Session: {session_id}]\n");
    for (role, text) in messages {
        output.push_str(&format!("[{role}] {text}\n"));
    }
    output
}

impl SourceAdapter for AntigravityAdapter {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn display_name(&self) -> &'static str {
        "Antigravity"
    }

    fn collect(&self, ctx: &CollectContext) -> Result<SourceCollection, parser::ParseError> {
        let settings = settings(ctx.config);
        if !settings.enabled {
            return Ok(SourceCollection {
                verbose_lines: vec![verbose::ANTIGRAVITY_DISABLED.to_string()],
                ..SourceCollection::default()
            });
        }

        let roots = self.resolve_roots(&settings);
        if roots.is_empty() {
            // Machines without Antigravity keep the same screen as before.
            return Ok(SourceCollection {
                excluded: vec![(EXCLUDED_PRINT_MODE, 0)],
                ..SourceCollection::default()
            });
        }

        let window = parser::local_date_to_utc_window(ctx.date)?;
        let mut stats = ScanStats::default();
        let mut conversations = load_conversations(&roots, window.start_utc, &mut stats);
        for conversation in conversations.values_mut() {
            merge_steps(&mut conversation.steps);
        }
        let histories = if settings.include_print_mode {
            HashMap::new()
        } else {
            load_histories(&conversations, &mut stats.unreadable)
        };

        let mut print_mode = 0;
        let mut entry_count = 0;
        let mut sessions = Vec::new();
        for (id, conversation) in &conversations {
            let day_steps: Vec<&TranscriptStep> = conversation
                .steps
                .iter()
                .filter(|step| {
                    is_conversation_step(step)
                        && step.created_at.is_some_and(|ts| window.contains(ts))
                })
                .collect();
            if day_steps.is_empty() {
                continue;
            }
            if !settings.include_print_mode && is_print_mode(id, conversation, &histories) {
                print_mode += 1;
                continue;
            }
            let messages = conversation_messages(&day_steps);
            if messages.is_empty() {
                continue;
            }

            entry_count += day_steps.len();
            let prompt_text = build_prompt(id, &messages);
            sessions.push(CollectedSession {
                task: analyzer::SessionTask {
                    source: Self::ID,
                    session_id: id.clone(),
                    estimated_tokens: analyzer::prompt::estimate_prompt_tokens(&prompt_text),
                    prompt_text,
                    messages,
                },
                first_activity: day_steps.iter().filter_map(|step| step.created_at).min(),
                last_activity: day_steps.iter().filter_map(|step| step.created_at).max(),
                token_usage: None,
            });
        }
        sessions.sort_by_key(|session| session.first_activity);

        let mut verbose_lines = vec![verbose::antigravity_scan(
            stats.found,
            stats.unmodified,
            stats.read,
        )];
        verbose_lines.extend(stats.unreadable);

        Ok(SourceCollection {
            roots,
            fingerprint: SourceFingerprint {
                session_count: sessions.len(),
                entry_count,
            },
            sessions,
            excluded: vec![(EXCLUDED_PRINT_MODE, print_mode)],
            verbose_lines,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    const CONV_A: &str = "aaaaaaaa-0000-4000-8000-000000000001";
    const CONV_B: &str = "bbbbbbbb-0000-4000-8000-000000000002";
    const CONV_C: &str = "cccccccc-0000-4000-8000-000000000003";

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ))
    }

    /// A past date: freshly written test files are always modified after it.
    fn date() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2025, 1, 15).expect("date")
    }

    /// RFC 3339 UTC timestamp for a local time in January 2025.
    fn local_ts(day: u32, hour: u32, minute: u32) -> String {
        chrono::Local
            .with_ymd_and_hms(2025, 1, day, hour, minute, 0)
            .earliest()
            .expect("local time")
            .with_timezone(&Utc)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string()
    }

    struct Layout {
        base: PathBuf,
    }

    impl Layout {
        fn new(prefix: &str) -> Self {
            Self {
                base: unique_temp_dir(prefix),
            }
        }

        fn brain(&self, data_dir: &str) -> PathBuf {
            self.base.join(data_dir).join(BRAIN_DIR)
        }

        fn agy(&self) -> PathBuf {
            self.brain(AGY_DATA_DIR)
        }

        fn app(&self) -> PathBuf {
            self.brain("antigravity")
        }

        fn ide(&self) -> PathBuf {
            self.brain("antigravity-ide")
        }

        fn write_history(&self, ids: &[&str]) {
            let dir = self.base.join(AGY_DATA_DIR);
            std::fs::create_dir_all(&dir).expect("agy dir");
            let lines: Vec<String> = ids
                .iter()
                .map(|id| {
                    json!({"display": "HISTORY_DISPLAY_MARKER", "conversationId": id}).to_string()
                })
                .collect();
            std::fs::write(dir.join(AGY_HISTORY_FILE), lines.join("\n")).expect("history");
        }
    }

    impl Drop for Layout {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.base).ok();
        }
    }

    fn logs_dir(brain: &Path, id: &str) -> PathBuf {
        brain.join(id).join(".system_generated").join("logs")
    }

    fn write_transcript(brain: &Path, id: &str, lines: &[String]) -> PathBuf {
        let dir = logs_dir(brain, id);
        std::fs::create_dir_all(&dir).expect("logs dir");
        let path = dir.join("transcript_full.jsonl");
        std::fs::write(&path, format!("{}\n", lines.join("\n"))).expect("transcript");
        path
    }

    fn user_input(index: u64, ts: &str, request: &str) -> String {
        json!({
            "step_index": index,
            "source": "USER_EXPLICIT",
            "type": "USER_INPUT",
            "status": "DONE",
            "created_at": ts,
            "content": format!(
                "<USER_REQUEST>\n{request}\n</USER_REQUEST>\n<ADDITIONAL_METADATA>\nMETADATA_MARKER /Users/x/project\n</ADDITIONAL_METADATA>"
            ),
        })
        .to_string()
    }

    fn planner(index: u64, ts: &str, thinking: &str, content: &str, tools: &[&str]) -> String {
        let tool_calls: Vec<serde_json::Value> = tools
            .iter()
            .map(|name| json!({"name": name, "args": {"CommandLine": "ARGS_MARKER"}}))
            .collect();
        json!({
            "step_index": index,
            "type": "PLANNER_RESPONSE",
            "status": "DONE",
            "created_at": ts,
            "thinking": thinking,
            "content": content,
            "tool_calls": tool_calls,
        })
        .to_string()
    }

    fn other_step(index: u64, ts: &str, step_type: &str, content: &str) -> String {
        json!({"step_index": index, "type": step_type, "created_at": ts, "content": content})
            .to_string()
    }

    /// One user input and one response on the target date.
    fn single_turn() -> Vec<String> {
        vec![
            user_input(0, &local_ts(15, 10, 0), "hello"),
            planner(1, &local_ts(15, 10, 1), "think", "answer", &[]),
        ]
    }

    fn two_turns() -> Vec<String> {
        vec![
            user_input(0, &local_ts(15, 10, 0), "first"),
            planner(1, &local_ts(15, 10, 1), "", "one", &[]),
            user_input(2, &local_ts(15, 10, 5), "second"),
            planner(3, &local_ts(15, 10, 6), "", "two", &[]),
        ]
    }

    fn collect_with(roots: Vec<PathBuf>, config: Option<&config::Config>) -> SourceCollection {
        AntigravityAdapter::with_roots(roots)
            .collect(&CollectContext {
                date: date(),
                config,
                include_automated: false,
            })
            .expect("collect")
    }

    fn collect(roots: Vec<PathBuf>) -> SourceCollection {
        collect_with(roots, None)
    }

    fn config_with(input_section: &str) -> config::Config {
        toml::from_str(&format!(
            "[llm]\nprovider = \"anthropic\"\nanthropic_api_key = \"sk-test\"\n\n\
             [output]\npath = \"/tmp/vault\"\n\n{input_section}"
        ))
        .expect("config")
    }

    fn session_ids(collection: &SourceCollection) -> Vec<&str> {
        collection
            .sessions
            .iter()
            .map(|session| session.task.session_id.as_str())
            .collect()
    }

    #[test]
    fn test_collect_sample_builds_fixed_prompt_text() {
        let layout = Layout::new("rwd_test_sources_antigravity_fixed_prompt");
        write_transcript(
            &layout.app(),
            CONV_A,
            &[
                user_input(0, &local_ts(15, 18, 29), "Why does the cache miss?"),
                planner(
                    1,
                    &local_ts(15, 18, 30),
                    "Check the key.",
                    "The timezone key changed.",
                    &["view_file", "run_command"],
                ),
                other_step(
                    2,
                    &local_ts(15, 18, 31),
                    "RUN_COMMAND",
                    "STEP_OUTPUT_MARKER",
                ),
                user_input(3, &local_ts(15, 18, 32), "Fix it."),
            ],
        );

        let collection = collect(vec![layout.app()]);

        assert_eq!(collection.sessions.len(), 1);
        let task = &collection.sessions[0].task;
        assert_eq!(task.source, AntigravityAdapter::ID);
        assert_eq!(task.session_id, CONV_A);
        assert_eq!(
            task.prompt_text,
            format!(
                "[Session: {CONV_A}]\n\
                 [USER] Why does the cache miss?\n\
                 [ASSISTANT] Check the key.\nThe timezone key changed.\n\
                 [TOOL] view_file\n\
                 [TOOL] run_command\n\
                 [USER] Fix it.\n"
            )
        );
        assert_eq!(
            task.estimated_tokens,
            analyzer::prompt::estimate_prompt_tokens(&task.prompt_text)
        );
        assert_eq!(
            collection.fingerprint,
            SourceFingerprint {
                session_count: 1,
                entry_count: 3,
            }
        );
        assert_eq!(collection.sessions[0].token_usage, None);
    }

    #[test]
    fn test_collect_drops_metadata_args_and_other_step_text() {
        let layout = Layout::new("rwd_test_sources_antigravity_markers");
        let no_tag = json!({
            "step_index": 3,
            "type": "USER_INPUT",
            "created_at": local_ts(15, 10, 3),
            "content": "untagged request",
        })
        .to_string();
        write_transcript(
            &layout.app(),
            CONV_A,
            &[
                user_input(0, &local_ts(15, 10, 0), "tagged request"),
                planner(1, &local_ts(15, 10, 1), "", "ok", &["run_command"]),
                other_step(2, &local_ts(15, 10, 2), "RUN_COMMAND", "RUN_MARKER"),
                no_tag,
                other_step(4, &local_ts(15, 10, 4), "BRAND_NEW_STEP", "NEW_TYPE_MARKER"),
                other_step(
                    5,
                    &local_ts(15, 10, 5),
                    "CONVERSATION_HISTORY",
                    "SUMMARY_MARKER",
                ),
            ],
        );

        let collection = collect(vec![layout.app()]);

        let task = &collection.sessions[0].task;
        for marker in [
            "METADATA_MARKER",
            "ARGS_MARKER",
            "RUN_MARKER",
            "NEW_TYPE_MARKER",
            "SUMMARY_MARKER",
        ] {
            assert!(!task.prompt_text.contains(marker), "{marker} leaked");
        }
        let users: Vec<&str> = task
            .messages
            .iter()
            .filter(|(role, _)| role == "USER")
            .map(|(_, text)| text.as_str())
            .collect();
        assert_eq!(users, vec!["tagged request", "untagged request"]);
        assert_eq!(collection.fingerprint.entry_count, 3);
    }

    #[test]
    fn test_collect_ignores_truncated_transcript_and_chunks() {
        let layout = Layout::new("rwd_test_sources_antigravity_other_files");
        let full = write_transcript(&layout.app(), CONV_A, &single_turn());
        let logs = full.parent().expect("logs dir").to_path_buf();
        let marker_line = user_input(5, &local_ts(15, 11, 0), "COPY_MARKER");
        std::fs::write(logs.join("transcript.jsonl"), &marker_line).expect("truncated copy");
        std::fs::create_dir_all(logs.join("chunks")).expect("chunks dir");
        std::fs::write(logs.join("chunks").join("chunk_0.jsonl"), &marker_line).expect("chunk");
        // A conversation that only has the truncated copy is not read at all.
        let only_copy = logs_dir(&layout.app(), CONV_B);
        std::fs::create_dir_all(&only_copy).expect("copy-only dir");
        std::fs::write(only_copy.join("transcript.jsonl"), &marker_line).expect("copy only");

        let collection = collect(vec![layout.app()]);

        assert_eq!(session_ids(&collection), vec![CONV_A]);
        assert!(
            !collection.sessions[0]
                .task
                .prompt_text
                .contains("COPY_MARKER")
        );
    }

    #[test]
    fn test_collect_keeps_only_steps_inside_local_date() {
        let layout = Layout::new("rwd_test_sources_antigravity_date_window");
        write_transcript(
            &layout.app(),
            CONV_A,
            &[
                user_input(0, &local_ts(14, 23, 59), "BEFORE_MARKER"),
                user_input(1, &local_ts(15, 0, 0), "at start"),
                planner(2, &local_ts(15, 23, 59), "", "at end", &[]),
                planner(3, &local_ts(16, 0, 0), "", "AFTER_MARKER", &[]),
            ],
        );

        let collection = collect(vec![layout.app()]);

        let session = &collection.sessions[0];
        assert!(!session.task.prompt_text.contains("BEFORE_MARKER"));
        assert!(!session.task.prompt_text.contains("AFTER_MARKER"));
        let ts = |s: String| s.parse::<DateTime<Utc>>().expect("timestamp");
        assert_eq!(session.first_activity, Some(ts(local_ts(15, 0, 0))));
        assert_eq!(session.last_activity, Some(ts(local_ts(15, 23, 59))));
        assert_eq!(collection.fingerprint.entry_count, 2);
    }

    #[test]
    fn test_collect_skips_files_not_modified_since_date_start() {
        let layout = Layout::new("rwd_test_sources_antigravity_mtime");
        // Steps fall inside the window, but the file claims it was last
        // written before the window: it must not even be opened.
        let path = write_transcript(&layout.app(), CONV_A, &single_turn());
        let window = parser::local_date_to_utc_window(date()).expect("window");
        let old = std::time::SystemTime::from(window.start_utc - chrono::Duration::hours(1));
        std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("open for mtime")
            .set_modified(old)
            .expect("set mtime");

        let collection = collect(vec![layout.app()]);

        assert!(collection.sessions.is_empty());
        assert_eq!(
            collection.verbose_lines[0],
            verbose::antigravity_scan(1, 1, 0)
        );
    }

    #[test]
    fn test_collect_recent_file_with_other_day_steps_has_no_session() {
        let layout = Layout::new("rwd_test_sources_antigravity_other_day");
        layout.write_history(&[]);
        // Modified now, but every step is from an earlier day: like the
        // August agy runs whose files were touched again on 2026-10-24.
        write_transcript(
            &layout.agy(),
            CONV_A,
            &[
                user_input(0, &local_ts(10, 9, 0), "old"),
                planner(1, &local_ts(10, 9, 1), "", "old answer", &[]),
            ],
        );

        let collection = collect(vec![layout.agy()]);

        assert!(collection.sessions.is_empty());
        assert_eq!(collection.excluded, vec![(EXCLUDED_PRINT_MODE, 0)]);
        assert_eq!(collection.fingerprint, SourceFingerprint::default());
    }

    #[test]
    fn test_collect_agy_print_mode_rule_by_history_and_input_count() {
        let layout = Layout::new("rwd_test_sources_antigravity_print_mode");
        layout.write_history(&[CONV_B]);
        // Not in history, one input: an `agy -p` run.
        write_transcript(&layout.agy(), CONV_A, &single_turn());
        // In history: interactive.
        write_transcript(&layout.agy(), CONV_B, &single_turn());
        // Not in history, but two inputs: interactive.
        write_transcript(&layout.agy(), CONV_C, &two_turns());

        let collection = collect(vec![layout.agy()]);

        assert_eq!(session_ids(&collection), vec![CONV_B, CONV_C]);
        assert_eq!(collection.excluded, vec![(EXCLUDED_PRINT_MODE, 1)]);
        assert_eq!(collection.fingerprint.session_count, 2);
        for session in &collection.sessions {
            assert!(!session.task.prompt_text.contains("HISTORY_DISPLAY_MARKER"));
        }
    }

    #[test]
    fn test_collect_include_print_mode_keeps_every_agy_conversation() {
        let layout = Layout::new("rwd_test_sources_antigravity_include_print_mode");
        layout.write_history(&[]);
        write_transcript(&layout.agy(), CONV_A, &single_turn());
        let config = config_with("[input.antigravity]\ninclude_print_mode = true\n");

        let collection = collect_with(vec![layout.agy()], Some(&config));

        assert_eq!(session_ids(&collection), vec![CONV_A]);
        assert_eq!(collection.excluded, vec![(EXCLUDED_PRINT_MODE, 0)]);
    }

    #[test]
    fn test_collect_missing_history_keeps_every_agy_conversation() {
        let layout = Layout::new("rwd_test_sources_antigravity_no_history");
        write_transcript(&layout.agy(), CONV_A, &single_turn());

        let collection = collect(vec![layout.agy()]);

        assert_eq!(session_ids(&collection), vec![CONV_A]);
        assert_eq!(collection.excluded, vec![(EXCLUDED_PRINT_MODE, 0)]);
        let history = layout.base.join(AGY_DATA_DIR).join(AGY_HISTORY_FILE);
        assert!(
            collection
                .verbose_lines
                .contains(&verbose::antigravity_history_missing(&history.display()))
        );
    }

    #[test]
    fn test_collect_app_folders_ignore_agy_history() {
        let layout = Layout::new("rwd_test_sources_antigravity_app_folders");
        layout.write_history(&[]);
        write_transcript(&layout.app(), CONV_A, &single_turn());
        write_transcript(&layout.ide(), CONV_B, &single_turn());

        let collection = collect(vec![layout.agy(), layout.app(), layout.ide()]);

        assert_eq!(session_ids(&collection), vec![CONV_A, CONV_B]);
        assert_eq!(collection.excluded, vec![(EXCLUDED_PRINT_MODE, 0)]);
        assert_eq!(collection.roots, vec![layout.app(), layout.ide()]);
    }

    #[test]
    fn test_collect_conversation_also_in_app_folder_is_not_print_mode() {
        let layout = Layout::new("rwd_test_sources_antigravity_agy_and_app");
        layout.write_history(&[]);
        write_transcript(&layout.agy(), CONV_A, &single_turn());
        write_transcript(&layout.app(), CONV_A, &single_turn());

        let collection = collect(vec![layout.agy(), layout.app()]);

        assert_eq!(session_ids(&collection), vec![CONV_A]);
        assert_eq!(collection.excluded, vec![(EXCLUDED_PRINT_MODE, 0)]);
    }

    #[test]
    fn test_collect_merges_same_conversation_across_roots_by_step_index() {
        let layout = Layout::new("rwd_test_sources_antigravity_merge");
        write_transcript(
            &layout.app(),
            CONV_A,
            &[
                user_input(0, &local_ts(15, 9, 0), "first"),
                planner(1, &local_ts(15, 9, 1), "", "one", &[]),
            ],
        );
        write_transcript(
            &layout.ide(),
            CONV_A,
            &[
                planner(1, &local_ts(15, 9, 1), "", "one", &[]),
                user_input(2, &local_ts(15, 9, 5), "second"),
            ],
        );

        let collection = collect(vec![layout.app(), layout.ide()]);

        assert_eq!(session_ids(&collection), vec![CONV_A]);
        let roles: Vec<&str> = collection.sessions[0]
            .task
            .messages
            .iter()
            .map(|(role, _)| role.as_str())
            .collect();
        assert_eq!(roles, vec!["USER", "ASSISTANT", "USER"]);
        assert_eq!(collection.fingerprint.entry_count, 3);
    }

    #[test]
    fn test_collect_broken_lines_keep_the_rest() {
        let layout = Layout::new("rwd_test_sources_antigravity_broken");
        let mut lines = single_turn();
        lines.insert(1, "{ broken".to_string());
        lines.push(r#"{"step_index":9,"type":"PLANNER_RESPONSE","content":"half"#.to_string());
        write_transcript(&layout.app(), CONV_A, &lines);

        let collection = collect(vec![layout.app()]);

        assert_eq!(session_ids(&collection), vec![CONV_A]);
        assert_eq!(collection.fingerprint.entry_count, 2);
    }

    #[cfg(unix)]
    #[test]
    fn test_collect_unreadable_file_keeps_other_files() {
        use std::os::unix::fs::PermissionsExt;

        let layout = Layout::new("rwd_test_sources_antigravity_unreadable");
        write_transcript(&layout.app(), CONV_A, &single_turn());
        let locked = write_transcript(&layout.app(), CONV_B, &single_turn());
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
            .expect("lock file");
        // Root can open any file; then there is nothing to check.
        let can_open = std::fs::File::open(&locked).is_ok();

        let collection = collect(vec![layout.app()]);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).ok();

        if can_open {
            return;
        }
        assert_eq!(session_ids(&collection), vec![CONV_A]);
        let prefix = verbose::antigravity_unreadable(&locked.display(), "");
        assert!(
            collection
                .verbose_lines
                .iter()
                .any(|line| line.starts_with(&prefix))
        );
    }

    #[test]
    fn test_collect_disabled_in_config_collects_nothing() {
        let layout = Layout::new("rwd_test_sources_antigravity_disabled");
        write_transcript(&layout.app(), CONV_A, &single_turn());
        let config = config_with("[input.antigravity]\nenabled = false\n");

        let collection = collect_with(vec![layout.app()], Some(&config));

        assert!(collection.roots.is_empty());
        assert!(collection.sessions.is_empty());
        assert_eq!(collection.fingerprint, SourceFingerprint::default());
        assert_eq!(
            collection.verbose_lines,
            vec![verbose::ANTIGRAVITY_DISABLED.to_string()]
        );
    }

    #[test]
    fn test_collect_config_without_antigravity_section_is_enabled() {
        let layout = Layout::new("rwd_test_sources_antigravity_default_enabled");
        write_transcript(&layout.app(), CONV_A, &single_turn());
        let config = config_with("[input.claude]\ninclude_automated = true\n");

        let collection = collect_with(vec![layout.app()], Some(&config));

        assert_eq!(session_ids(&collection), vec![CONV_A]);
    }

    #[test]
    fn test_collect_missing_roots_returns_empty_collection() {
        let layout = Layout::new("rwd_test_sources_antigravity_missing");

        let collection = collect(vec![layout.agy(), layout.app()]);

        assert!(collection.roots.is_empty());
        assert!(collection.sessions.is_empty());
        assert!(collection.verbose_lines.is_empty());
    }

    #[test]
    fn test_agy_history_path_only_for_agy_brain_roots() {
        let base = PathBuf::from("/home/u/.gemini");
        assert_eq!(
            agy_history_path(&base.join("antigravity-cli/brain")),
            Some(base.join("antigravity-cli/history.jsonl"))
        );
        assert_eq!(agy_history_path(&base.join("antigravity/brain")), None);
        assert_eq!(agy_history_path(&base.join("antigravity-ide/brain")), None);
    }
}
