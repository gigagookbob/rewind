// Antigravity (agy CLI, Antigravity app, Antigravity IDE) conversation log parser.
//
// Reads `brain/<conversation id>/.system_generated/logs/transcript_full.jsonl`
// (one conversation step per line) and the conversation ids in agy's
// `history.jsonl`. The format has no public documentation; the field notes
// below come from reading this machine's logs.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::Path;
use std::time::Duration;

/// User prompt step. Verified: all 86 USER_INPUT lines wrap the typed text
/// in <USER_REQUEST>.
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
    #[serde(skip)]
    pub source_location: Option<super::SourceLocation>,
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
    for (line_num, line) in reader.split(b'\n').enumerate() {
        let line = line?;
        let Ok(text) = std::str::from_utf8(&line) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        if let Ok(mut step) = serde_json::from_str::<TranscriptStep>(text) {
            step.source_location = Some(super::SourceLocation::new(path, line_num + 1));
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

/// Only `conversationId` and `workspace` are read; the typed text (`display`)
/// is never deserialized.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct HistoryLine {
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
    workspace: Option<String>,
}

/// Conversation ids and workspace hints extracted from `agy`'s `history.jsonl`.
#[derive(Debug, Default)]
pub struct AgyHistory {
    pub conversation_ids: HashSet<String>,
    pub workspaces: HashMap<String, Vec<String>>,
}

/// Reads conversation ids and workspace paths from `agy`'s `history.jsonl`.
///
/// Returns `None` when the file is missing or unreadable. Broken lines are skipped.
pub fn read_agy_history(path: &Path) -> Option<AgyHistory> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    let mut history = AgyHistory::default();

    for line in reader.split(b'\n') {
        let line = line.ok()?;
        let Ok(text) = std::str::from_utf8(&line) else {
            continue;
        };
        if let Ok(parsed) = serde_json::from_str::<HistoryLine>(text)
            && let Some(id) = parsed.conversation_id
            && !id.is_empty()
        {
            if let Some(ws) = parsed.workspace.map(|w| w.trim().to_string())
                && !ws.is_empty()
            {
                let entries = history.workspaces.entry(id.clone()).or_default();
                if !entries.contains(&ws) {
                    entries.push(ws);
                }
            }
            history.conversation_ids.insert(id);
        }
    }

    Some(history)
}

/// Normalizes a `file://` URI or local path into a local absolute directory path.
pub fn normalize_workspace_entry(raw: &str, is_wsl: bool) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = if trimmed.starts_with("file://") {
        file_uri_to_local_path(trimmed)?
    } else if trimmed.contains("://") || trimmed.contains('\0') {
        return None;
    } else {
        trimmed.to_string()
    };

    if is_wsl && let Some(wsl) = windows_drive_to_wsl(&path) {
        return Some(wsl);
    }
    Some(path)
}

fn file_uri_to_local_path(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let raw_path = if let Some(after_host) = rest.strip_prefix("localhost/") {
        format!("/{after_host}")
    } else if rest.starts_with('/') {
        rest.to_string()
    } else {
        return None;
    };

    let decoded = percent_decode_utf8(&raw_path)?;
    let bytes = decoded.as_bytes();
    if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
        && (bytes.len() == 3 || bytes[3] == b'/' || bytes[3] == b'\\')
    {
        return Some(decoded[1..].to_string());
    }
    Some(decoded)
}

fn percent_decode_utf8(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = (*bytes.get(i + 1)? as char).to_digit(16)?;
            let lo = (*bytes.get(i + 2)? as char).to_digit(16)?;
            out.push(((hi << 4) | lo) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded = String::from_utf8(out).ok()?;
    (!decoded.contains('\0')).then_some(decoded)
}

fn windows_drive_to_wsl(path: &str) -> Option<String> {
    let mut chars = path.chars();
    let drive = chars.next()?;
    if !drive.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }
    match chars.next() {
        Some('/') | Some('\\') => {
            let rest = chars.as_str().replace('\\', "/");
            Some(format!(
                "/mnt/{}/{}",
                drive.to_ascii_lowercase(),
                rest.trim_start_matches('/')
            ))
        }
        None => Some(format!("/mnt/{}", drive.to_ascii_lowercase())),
        Some(_) => None,
    }
}

/// Extracts `file://` workspace URIs from `trajectory_metadata_blob` protobuf bytes.
///
/// Outer repeated field 1 holds workspace metadata entries whose inner field 1 is
/// the workspace URI string. Malformed or truncated bytes stop decoding safely.
pub fn extract_workspace_uris_from_metadata_blob(bytes: &[u8]) -> Vec<String> {
    let mut uris = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let Some(tag) = decode_varint(bytes, &mut pos) else {
            break;
        };
        let field_no = tag >> 3;
        let wire_type = tag & 7;
        if field_no == 1 && wire_type == 2 {
            let Some(entry_bytes) = read_length_delimited(bytes, &mut pos) else {
                break;
            };
            collect_inner_file_uris(entry_bytes, &mut uris);
        } else if skip_protobuf_field(wire_type, bytes, &mut pos).is_none() {
            break;
        }
    }
    uris
}

fn collect_inner_file_uris(bytes: &[u8], out: &mut Vec<String>) {
    let mut pos = 0;
    while pos < bytes.len() {
        let Some(tag) = decode_varint(bytes, &mut pos) else {
            break;
        };
        let field_no = tag >> 3;
        let wire_type = tag & 7;
        if field_no == 1 && wire_type == 2 {
            let Some(raw) = read_length_delimited(bytes, &mut pos) else {
                break;
            };
            if let Ok(text) = std::str::from_utf8(raw)
                && text.starts_with("file://")
            {
                out.push(text.to_string());
            }
        } else if skip_protobuf_field(wire_type, bytes, &mut pos).is_none() {
            break;
        }
    }
}

fn decode_varint(bytes: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0_u64;
    let mut shift = 0_u32;
    for _ in 0..10 {
        let byte = *bytes.get(*pos)?;
        *pos += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn read_length_delimited<'a>(bytes: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    let len = usize::try_from(decode_varint(bytes, pos)?).ok()?;
    let end = pos.checked_add(len)?;
    let slice = bytes.get(*pos..end)?;
    *pos = end;
    Some(slice)
}

fn skip_protobuf_field(wire_type: u64, bytes: &[u8], pos: &mut usize) -> Option<()> {
    match wire_type {
        0 => {
            decode_varint(bytes, pos)?;
            Some(())
        }
        1 => {
            let end = pos.checked_add(8)?;
            bytes.get(*pos..end)?;
            *pos = end;
            Some(())
        }
        2 => {
            read_length_delimited(bytes, pos)?;
            Some(())
        }
        5 => {
            let end = pos.checked_add(4)?;
            bytes.get(*pos..end)?;
            *pos = end;
            Some(())
        }
        _ => None,
    }
}

fn open_readonly_sqlite(path: &Path) -> Result<Option<Connection>, super::ParseError> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() == 0 => return Ok(None),
        Ok(_) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_millis(250))?;
    Ok(Some(conn))
}

/// Reads `workspace_uris` for every conversation row in `conversation_summaries.db`.
///
/// Returns `Ok(None)` when the file does not exist or is empty so callers can fall back.
pub fn read_summary_db_workspaces(
    path: &Path,
) -> Result<Option<HashMap<String, Vec<String>>>, super::ParseError> {
    let Some(conn) = open_readonly_sqlite(path)? else {
        return Ok(None);
    };
    let mut stmt =
        conn.prepare("SELECT conversation_id, workspace_uris FROM conversation_summaries")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, Option<String>>(1)?,
        ))
    })?;

    let mut result = HashMap::new();
    for row in rows {
        let (id_opt, uris_opt) = row?;
        let Some(id) = id_opt.filter(|s| !s.is_empty()) else {
            continue;
        };
        let raw = uris_opt.unwrap_or_default();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            result.insert(id, Vec::new());
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(trimmed) {
            let filtered = parsed
                .into_iter()
                .filter(|s| !s.trim().is_empty())
                .collect();
            result.insert(id, filtered);
        }
    }
    Ok(Some(result))
}

/// Reads workspace URIs from `trajectory_metadata_blob` (`id = 'main'`) in `conversations/<id>.db`.
pub fn read_conversation_db_workspaces(
    path: &Path,
) -> Result<Option<Vec<String>>, super::ParseError> {
    let Some(conn) = open_readonly_sqlite(path)? else {
        return Ok(None);
    };
    let blob_opt: Option<Option<Vec<u8>>> = conn
        .query_row(
            "SELECT data FROM trajectory_metadata_blob WHERE id = 'main' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let uris = match blob_opt {
        Some(Some(blob)) if !blob.is_empty() => extract_workspace_uris_from_metadata_blob(&blob),
        _ => Vec::new(),
    };
    Ok(Some(uris))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_location_antigravity_ignores_payload_location() {
        let path =
            std::env::temp_dir().join(format!("rwd-agy-location-{}.jsonl", std::process::id()));
        std::fs::write(&path, "\ninvalid\n{\"type\":\"USER_INPUT\",\"content\":\"hello\",\"source_location\":{\"path\":\"fake\",\"line\":99}}\n").unwrap();
        let steps = parse_transcript_file(&path).unwrap();
        let location = steps[0].source_location.as_ref().unwrap();
        assert_eq!(location.path, path);
        assert_eq!(location.line, 3);
        std::fs::remove_file(path).unwrap();
    }
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

        let history = read_agy_history(&path).expect("history ids");
        std::fs::remove_dir_all(&dir).ok();

        let mut ids: Vec<String> = history.conversation_ids.into_iter().collect();
        ids.sort();
        assert_eq!(ids, vec!["conv-a".to_string(), "conv-b".to_string()]);
    }

    #[test]
    fn test_read_history_conversation_ids_missing_file_returns_none() {
        let dir = unique_temp_dir("rwd_test_parser_antigravity_history_missing");
        let history = read_agy_history(&dir.join("history.jsonl"));
        std::fs::remove_dir_all(&dir).ok();

        assert!(history.is_none());
    }

    #[test]
    fn test_normalize_workspace_entry_decodes_file_uris_and_wsl_drives() {
        assert_eq!(
            normalize_workspace_entry("file:///Users/max/Workspace/hobby/rewind", false),
            Some("/Users/max/Workspace/hobby/rewind".to_string())
        );
        assert_eq!(
            normalize_workspace_entry(
                "file:///Users/max/%ED%95%9C%EA%B8%80%20%ED%8F%B4%EB%8D%94/app",
                false
            ),
            Some("/Users/max/한글 폴더/app".to_string())
        );
        assert_eq!(
            normalize_workspace_entry("file://localhost/Users/max/app", false),
            Some("/Users/max/app".to_string())
        );
        assert_eq!(
            normalize_workspace_entry("file://fileserver/share/app", false),
            None
        );
        assert_eq!(
            normalize_workspace_entry("https://example.com/app", false),
            None
        );
        assert_eq!(normalize_workspace_entry("file:///bad%2Gpath", false), None);
        assert_eq!(normalize_workspace_entry("file:///bad%00path", false), None);
        assert_eq!(
            normalize_workspace_entry("file:///C:/Users/me/repo", true),
            Some("/mnt/c/Users/me/repo".to_string())
        );
        assert_eq!(
            normalize_workspace_entry(r"C:\Users\me\repo", true),
            Some("/mnt/c/Users/me/repo".to_string())
        );
        assert_eq!(
            normalize_workspace_entry("file:///C:/Users/me/repo", false),
            Some("C:/Users/me/repo".to_string())
        );
    }

    #[test]
    fn test_extract_workspace_uris_from_metadata_blob_handles_valid_and_malformed_bytes() {
        fn encode_ld(field_no: u64, payload: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            let mut tag = (field_no << 3) | 2;
            while tag >= 0x80 {
                out.push((tag as u8 & 0x7f) | 0x80);
                tag >>= 7;
            }
            out.push(tag as u8);
            let mut len = payload.len();
            while len >= 0x80 {
                out.push((len as u8 & 0x7f) | 0x80);
                len >>= 7;
            }
            out.push(len as u8);
            out.extend_from_slice(payload);
            out
        }

        let mut valid = Vec::new();
        valid.extend_from_slice(&encode_ld(1, &encode_ld(1, b"file:///repo-one")));
        valid.extend_from_slice(&encode_ld(1, &encode_ld(1, b"file:///repo-two")));
        valid.extend_from_slice(&[0x10, 0x01]);
        assert_eq!(
            extract_workspace_uris_from_metadata_blob(&valid),
            vec![
                "file:///repo-one".to_string(),
                "file:///repo-two".to_string()
            ]
        );

        for broken in [b"".as_slice(), b"\x80", b"\x0a\xff\x01", b"\x0f"] {
            assert!(extract_workspace_uris_from_metadata_blob(broken).is_empty());
        }
    }

    #[test]
    fn test_read_summary_and_conversation_dbs_in_readonly_mode() {
        let dir = unique_temp_dir("rwd_test_parser_antigravity_dbs");
        let missing = dir.join("missing.db");
        assert!(read_summary_db_workspaces(&missing).unwrap().is_none());
        assert!(!missing.exists());

        let empty_db = dir.join("empty.db");
        std::fs::write(&empty_db, b"").unwrap();
        assert!(read_summary_db_workspaces(&empty_db).unwrap().is_none());
        assert!(
            read_conversation_db_workspaces(&empty_db)
                .unwrap()
                .is_none()
        );

        let summary_db = dir.join("conversation_summaries.db");
        let conn = rusqlite::Connection::open(&summary_db).unwrap();
        conn.execute(
            "CREATE TABLE conversation_summaries (conversation_id TEXT PRIMARY KEY, workspace_uris TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO conversation_summaries VALUES ('c1', '[\"file:///Users/max/app\"]'), ('c2', '')",
            [],
        )
        .unwrap();
        drop(conn);

        let summaries = read_summary_db_workspaces(&summary_db).unwrap().unwrap();
        assert_eq!(
            summaries.get("c1"),
            Some(&vec!["file:///Users/max/app".to_string()])
        );
        assert_eq!(summaries.get("c2"), Some(&Vec::new()));

        let conv_db = dir.join("c1.db");
        let conn = rusqlite::Connection::open(&conv_db).unwrap();
        conn.execute(
            "CREATE TABLE trajectory_metadata_blob (id TEXT PRIMARY KEY, data BLOB)",
            [],
        )
        .unwrap();
        let mut blob = vec![0x0a, 0x16, 0x0a, 0x14];
        blob.extend_from_slice(b"file:///Users/max/a1");
        conn.execute(
            "INSERT INTO trajectory_metadata_blob (id, data) VALUES ('main', ?1)",
            [blob],
        )
        .unwrap();
        drop(conn);

        assert_eq!(
            read_conversation_db_workspaces(&conv_db).unwrap(),
            Some(vec!["file:///Users/max/a1".to_string()])
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_agy_history_collects_workspaces_without_reading_display() {
        let dir = unique_temp_dir("rwd_test_parser_antigravity_history_ws");
        let path = dir.join("history.jsonl");
        std::fs::write(
            &path,
            concat!(
                r#"{"display":"DISPLAY_SECRET","workspace":"/Users/max/repo-a","conversationId":"conv-a"}"#,
                "\n",
                r#"{"display":"MORE","workspace":"/Users/max/repo-a","conversationId":"conv-a"}"#,
                "\n",
                r#"{"conversationId":"conv-b","workspace":""}"#,
                "\n",
            ),
        )
        .unwrap();

        let history = read_agy_history(&path).expect("agy history");
        std::fs::remove_dir_all(&dir).ok();

        assert!(history.conversation_ids.contains("conv-a"));
        assert!(history.conversation_ids.contains("conv-b"));
        assert_eq!(
            history.workspaces.get("conv-a"),
            Some(&vec!["/Users/max/repo-a".to_string()])
        );
        assert!(!history.workspaces.contains_key("conv-b"));
        assert!(!format!("{history:?}").contains("DISPLAY_SECRET"));
    }
}
