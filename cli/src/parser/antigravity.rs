// Antigravity (agy CLI, Antigravity app, Antigravity IDE) conversation log parser.
//
// Reads `brain/<conversation id>/.system_generated/logs/transcript_full.jsonl`
// (one conversation step per line) and the conversation ids in agy's
// `history.jsonl`. The format has no public documentation; the field notes
// below come from reading this machine's logs (SPEC.md, Antigravity adapter).

use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashSet;
use std::io::BufRead;
use std::path::Path;

/// User prompt step. Verified: all 86 USER_INPUT lines wrap the typed text
/// in <USER_REQUEST> (SPEC.md, Antigravity line structure).
pub const STEP_USER_INPUT: &str = "USER_INPUT";
/// Model response step: `content` = answer, `thinking`, `tool_calls[].name`.
/// Verified against SQLite step payloads (100 answers, 154 thoughts).
pub const STEP_PLANNER_RESPONSE: &str = "PLANNER_RESPONSE";

/// Wraps the text the user typed inside a USER_INPUT step. Verified on 86 of
/// 86 lines; everything outside it (`<ADDITIONAL_METADATA>`,
/// `<USER_SETTINGS_CHANGE>`, `<PLAN>`) is app-added context and is dropped.
const USER_REQUEST_OPEN: &str = "<USER_REQUEST>";
const USER_REQUEST_CLOSE: &str = "</USER_REQUEST>";

/// One `transcript_full.jsonl` line (one conversation step).
///
/// Every field has a default so missing or new fields never stop parsing.
/// Fields rwd does not use (`source`, `status`, `exit_code`, ...) are ignored.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct TranscriptStep {
    /// Step number inside the conversation; this order is the conversation order.
    pub step_index: Option<u64>,
    #[serde(rename = "type")]
    pub step_type: String,
    /// UTC ISO 8601, e.g. `2026-08-23T04:21:31Z`.
    pub created_at: Option<DateTime<Utc>>,
    pub content: Option<String>,
    /// Model reasoning. Only PLANNER_RESPONSE carries it.
    pub thinking: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

/// Tool call made by the model.
///
/// `args` is deliberately not deserialized so tool arguments can never reach
/// the analysis prompt.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ToolCall {
    pub name: String,
}

/// Reads every parseable step of one `transcript_full.jsonl`.
///
/// Broken lines and a half-written last line (agy or the app may still be
/// appending) are skipped without a warning. I/O errors such as a missing
/// file or denied permission are returned so the adapter can report them.
pub fn parse_transcript_file(path: &Path) -> Result<Vec<TranscriptStep>, super::ParseError> {
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut steps = Vec::new();

    // Split on raw bytes: a line cut in the middle of a multi-byte character
    // must only drop that line, not fail the whole file.
    for line in reader.split(b'\n') {
        let line = line?;
        let Ok(text) = std::str::from_utf8(&line) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        if let Ok(step) = serde_json::from_str::<TranscriptStep>(text) {
            steps.push(step);
        }
    }

    Ok(steps)
}

/// Returns the text inside `<USER_REQUEST>`, or the whole content when the
/// tag is missing.
pub fn extract_user_request(content: &str) -> &str {
    let Some(start) = content.find(USER_REQUEST_OPEN) else {
        return content.trim();
    };
    let body = &content[start + USER_REQUEST_OPEN.len()..];
    let end = body.find(USER_REQUEST_CLOSE).unwrap_or(body.len());
    body[..end].trim()
}

/// Only `conversationId` is read; the typed text (`display`) is never
/// deserialized.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HistoryLine {
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
}

/// Reads the conversation ids recorded in agy's `history.jsonl`.
///
/// Returns `None` when the file is missing or unreadable, so the caller can
/// tell "cannot detect `agy -p` runs" apart from "this id is not in history".
/// Broken lines are skipped.
pub fn read_history_conversation_ids(path: &Path) -> Option<HashSet<String>> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    let mut ids = HashSet::new();

    for line in reader.split(b'\n') {
        let line = line.ok()?;
        let Ok(text) = std::str::from_utf8(&line) else {
            continue;
        };
        if let Ok(HistoryLine {
            conversation_id: Some(id),
        }) = serde_json::from_str::<HistoryLine>(text)
            && !id.is_empty()
        {
            ids.insert(id);
        }
    }

    Some(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn test_extract_user_request_with_tag_returns_inner_text_only() {
        let content = "<USER_REQUEST>\nfix the cache key\n</USER_REQUEST>\n\
                       <ADDITIONAL_METADATA>\nMETA_MARKER_7f3a /Users/x/project\n</ADDITIONAL_METADATA>\n\
                       <USER_SETTINGS_CHANGE>SETTINGS_MARKER</USER_SETTINGS_CHANGE>";

        let text = extract_user_request(content);

        assert_eq!(text, "fix the cache key");
        assert!(!text.contains("META_MARKER_7f3a"));
        assert!(!text.contains("SETTINGS_MARKER"));
    }

    #[test]
    fn test_extract_user_request_without_tag_returns_whole_content() {
        assert_eq!(extract_user_request("  plain request  "), "plain request");
    }

    #[test]
    fn test_parse_transcript_file_reads_known_fields_and_skips_args() {
        let dir = unique_temp_dir("rwd_test_parser_antigravity_fields");
        let path = dir.join("transcript_full.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-08-23T04:21:31Z","content":"<USER_REQUEST>hi</USER_REQUEST>"}"#,
                "\n",
                r#"{"step_index":1,"type":"PLANNER_RESPONSE","created_at":"2026-08-23T04:21:40Z","content":"answer","thinking":"plan","tool_calls":[{"name":"run_command","args":{"CommandLine":"ARGS_MARKER"}}],"input_tokens":10}"#,
                "\n",
            ),
        )
        .expect("write transcript");

        let steps = parse_transcript_file(&path).expect("parse");
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].step_index, Some(0));
        assert_eq!(steps[0].step_type, STEP_USER_INPUT);
        assert_eq!(
            steps[0].created_at,
            Some("2026-08-23T04:21:31Z".parse().expect("timestamp"))
        );
        assert_eq!(steps[1].step_type, STEP_PLANNER_RESPONSE);
        assert_eq!(steps[1].content.as_deref(), Some("answer"));
        assert_eq!(steps[1].thinking.as_deref(), Some("plan"));
        assert_eq!(
            steps[1].tool_calls,
            vec![ToolCall {
                name: "run_command".to_string()
            }]
        );
        assert!(!format!("{steps:?}").contains("ARGS_MARKER"));
    }

    #[test]
    fn test_parse_transcript_file_with_broken_lines_keeps_the_rest() {
        let dir = unique_temp_dir("rwd_test_parser_antigravity_broken");
        let path = dir.join("transcript_full.jsonl");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(br#"{"step_index":0,"type":"USER_INPUT","content":"a"}"#);
        bytes.extend_from_slice(b"\nnot json at all\n");
        bytes.extend_from_slice(br#"{"step_index":1,"type":"BRAND_NEW_STEP","new_field":{"x":1}}"#);
        bytes.extend_from_slice(b"\n\n");
        bytes.extend_from_slice(br#"{"step_index":2,"type":"PLANNER_RESPONSE","content":"b"}"#);
        bytes.push(b'\n');
        // Half-written last line, cut inside a multi-byte character.
        bytes.extend_from_slice(
            r#"{"step_index":3,"type":"PLANNER_RESPONSE","content":"한"#.as_bytes(),
        );
        bytes.truncate(bytes.len() - 1);
        std::fs::write(&path, bytes).expect("write transcript");

        let steps = parse_transcript_file(&path).expect("parse");
        std::fs::remove_dir_all(&dir).ok();

        let indices: Vec<Option<u64>> = steps.iter().map(|s| s.step_index).collect();
        assert_eq!(indices, vec![Some(0), Some(1), Some(2)]);
        assert_eq!(steps[1].step_type, "BRAND_NEW_STEP");
    }

    #[test]
    fn test_parse_transcript_file_missing_file_returns_error() {
        let dir = unique_temp_dir("rwd_test_parser_antigravity_missing");
        let result = parse_transcript_file(&dir.join("transcript_full.jsonl"));
        std::fs::remove_dir_all(&dir).ok();

        assert!(result.is_err());
    }

    #[test]
    fn test_read_history_conversation_ids_returns_ids_without_display_text() {
        let dir = unique_temp_dir("rwd_test_parser_antigravity_history");
        let path = dir.join("history.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"display":"DISPLAY_MARKER","timestamp":1,"workspace":"/w","conversationId":"conv-a","type":"prompt"}"#,
                "\n",
                "broken line\n",
                r#"{"display":"no id here","timestamp":2}"#,
                "\n",
                r#"{"conversationId":"conv-b"}"#,
                "\n",
            ),
        )
        .expect("write history");

        let ids = read_history_conversation_ids(&path).expect("history ids");
        std::fs::remove_dir_all(&dir).ok();

        let mut ids: Vec<String> = ids.into_iter().collect();
        ids.sort();
        assert_eq!(ids, vec!["conv-a".to_string(), "conv-b".to_string()]);
    }

    #[test]
    fn test_read_history_conversation_ids_missing_file_returns_none() {
        let dir = unique_temp_dir("rwd_test_parser_antigravity_history_missing");
        let ids = read_history_conversation_ids(&dir.join("history.jsonl"));
        std::fs::remove_dir_all(&dir).ok();

        assert!(ids.is_none());
    }
}
