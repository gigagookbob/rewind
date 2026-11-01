// Codex CLI non-interactive provider backend.
//
// Runs `codex exec` with JSONL output and parses the final agent message.

use serde::Deserialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{AnalyzerError, ApiUsage};

/// Calls Codex for JSON analysis output.
pub(super) async fn call_codex_api(
    prompt: &str,
    max_tokens: u32,
    model: &str,
    reasoning_effort: &str,
    json_output: bool,
) -> Result<(String, ApiUsage), AnalyzerError> {
    let schema_path = if json_output {
        Some(write_analysis_schema_file()?)
    } else {
        None
    };
    let result = run_codex_exec(
        prompt,
        schema_path.as_deref(),
        max_tokens,
        model,
        reasoning_effort,
    )
    .await;
    if let Some(path) = schema_path {
        let _ = std::fs::remove_file(path);
    }
    result
}

pub(super) fn compose_prompt(system_prompt: &str, conversation_text: &str) -> String {
    format!("[System Instructions]\n{system_prompt}\n\n[Conversation]\n{conversation_text}")
}

/// Writes JSON schema for analysis output to a temporary file.
/// Codex reads schemas from file paths (`--output-schema`), so we create one per request.
fn write_analysis_schema_file() -> Result<PathBuf, AnalyzerError> {
    let now_nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "rwd-analysis-schema-{}-{now_nanos}.json",
        std::process::id()
    ));
    std::fs::write(&path, ANALYSIS_OUTPUT_SCHEMA)?;
    Ok(path)
}

/// Structured output schema for insight extraction.
const ANALYSIS_OUTPUT_SCHEMA: &str = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "additionalProperties": false,
  "$defs": {
    "evidence": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["message_id", "quote"],
        "properties": {
          "message_id": { "type": "string" },
          "quote": { "type": "string" }
        }
      }
    }
  },
  "required": ["sessions"],
  "properties": {
    "sessions": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["session_id", "work_summary", "decisions", "curiosities", "corrections", "til", "task_title", "questions", "follow_ups"],
        "properties": {
          "session_id": { "type": "string" },
          "work_summary": { "type": "string" },
          "task_title": {"type": ["object", "null"], "additionalProperties": false, "required": ["text", "evidence"], "properties": {"text": {"type": "string"}, "evidence": {"$ref": "#/$defs/evidence"}}},
          "questions": {"type": "array", "items": {"type": "object", "additionalProperties": false, "required": ["question", "evidence", "updates"], "properties": {"question": {"type": "string"}, "evidence": {"$ref": "#/$defs/evidence"}, "updates": {"type": "array", "items": {"type": "object", "additionalProperties": false, "required": ["kind", "detail", "evidence"], "properties": {"kind": {"type": "string", "enum": ["answer", "resolved", "open", "unknown"]}, "detail": {"type": "string"}, "evidence": {"$ref": "#/$defs/evidence"}}}}}}},
          "follow_ups": {"type": "array", "items": {"type": "object", "additionalProperties": false, "required": ["task", "origin", "evidence", "updates"], "properties": {"task": {"type": "string"}, "origin": {"type": "string", "enum": ["user_request", "ai_suggestion", "unknown"]}, "evidence": {"$ref": "#/$defs/evidence"}, "updates": {"type": "array", "items": {"type": "object", "additionalProperties": false, "required": ["kind", "detail", "evidence"], "properties": {"kind": {"type": "string", "enum": ["accepted", "completed", "withdrawn", "unknown"]}, "detail": {"type": "string"}, "evidence": {"$ref": "#/$defs/evidence"}}}}}}},
          "decisions": {
            "type": "array",
            "items": {
              "type": "object",
              "additionalProperties": false,
              "required": ["what", "why", "evidence"],
              "properties": {
                "what": { "type": "string" },
                "why": { "type": "string" },
                "evidence": { "$ref": "#/$defs/evidence" }
              }
            }
          },
          "curiosities": { "type": "array", "items": { "type": "string" } },
          "corrections": {
            "type": "array",
            "items": {
              "type": "object",
              "additionalProperties": false,
              "required": ["model_said", "user_corrected", "kind", "evidence"],
              "properties": {
                "model_said": { "type": "string" },
                "user_corrected": { "type": "string" },
                "kind": { "type": "string", "enum": ["factual_error", "preference_change", "unknown"] },
                "evidence": { "$ref": "#/$defs/evidence" }
              }
            }
          },
          "til": {
            "type": "array",
            "items": {
              "type": "object",
              "additionalProperties": false,
              "required": ["title", "detail", "evidence"],
              "properties": {
                "title": { "type": "string" },
                "detail": { "type": "string" },
                "evidence": { "$ref": "#/$defs/evidence" }
              }
            }
          }
        }
      }
    }
  }
}"##;

async fn run_codex_exec(
    prompt: &str,
    schema_path: Option<&Path>,
    max_tokens: u32,
    model: &str,
    reasoning_effort: &str,
) -> Result<(String, ApiUsage), AnalyzerError> {
    let prompt = prompt.to_string();
    let schema_path = schema_path.map(|path| path.to_path_buf());
    let model = model.to_string();
    let reasoning_effort = reasoning_effort.to_string();

    tokio::task::spawn_blocking(move || {
        run_codex_exec_blocking(
            &prompt,
            schema_path.as_deref(),
            max_tokens,
            &model,
            &reasoning_effort,
        )
        .map_err(|err| err.to_string())
    })
    .await
    .map_err(|join_err| format!("Codex exec join error: {join_err}"))?
    .map_err(Into::into)
}

fn build_codex_exec_command(
    schema_path: Option<&Path>,
    max_tokens: u32,
    model: &str,
    reasoning_effort: &str,
) -> Command {
    let temp_dir = std::env::temp_dir();

    let mut cmd = Command::new("codex");
    cmd.arg("exec")
        .arg("--json")
        .arg("--color")
        .arg("never")
        .arg("--ephemeral")
        .arg("--skip-git-repo-check")
        .arg("-s")
        .arg("read-only")
        .arg("-C")
        .arg(&temp_dir)
        .arg("-c")
        .arg(format!("model_reasoning_effort={reasoning_effort}"))
        .arg("-c")
        .arg(format!("model_max_output_tokens={max_tokens}"));

    if let Some(path) = schema_path {
        cmd.arg("--output-schema").arg(path);
    }

    if model != crate::config::DEFAULT_CODEX_MODEL && !model.trim().is_empty() {
        cmd.arg("--model").arg(model);
    }

    cmd.arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    cmd
}

fn run_codex_exec_blocking(
    prompt: &str,
    schema_path: Option<&Path>,
    max_tokens: u32,
    model: &str,
    reasoning_effort: &str,
) -> Result<(String, ApiUsage), AnalyzerError> {
    let mut cmd = build_codex_exec_command(schema_path, max_tokens, model, reasoning_effort);

    let mut child = cmd.spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(prompt.as_bytes())?;
    }
    let output = child.wait_with_output()?;

    parse_output(output)
}

pub(super) fn parse_output(
    output: std::process::Output,
) -> Result<(String, ApiUsage), AnalyzerError> {
    if !output.status.success() {
        if is_rate_limited(
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        ) {
            return Err("Codex exec failed: HTTP 429 (rate limited); response body omitted".into());
        }
        return Err(format!("Codex exec failed ({})", output.status).into());
    }

    let stdout = String::from_utf8(output.stdout)?;
    parse_jsonl_events(&stdout)
}

/// Preserve only a retry classification, never the untrusted error message.
fn is_rate_limited(stdout: &str, stderr: &str) -> bool {
    let limited = |message: &str| {
        message
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|s| s == "429")
            || message.to_ascii_lowercase().contains("rate limit")
    };
    stdout.lines().any(|line| {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            return false;
        };
        match event["type"].as_str() {
            Some("turn.failed") => event["error"]["message"].as_str().is_some_and(limited),
            Some("error") => event["message"].as_str().is_some_and(limited),
            _ => false,
        }
    }) || limited(stderr)
}

fn parse_jsonl_events(stdout: &str) -> Result<(String, ApiUsage), AnalyzerError> {
    let mut last_message: Option<String> = None;
    let mut usage = ApiUsage::default();

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<ExecEvent>(line) else {
            continue;
        };

        if event.event_type == "item.completed"
            && let Some(item) = event.item
            && item.item_type == "agent_message"
            && let Some(text) = item.text
        {
            last_message = Some(text);
        }

        if event.event_type == "turn.completed"
            && let Some(u) = event.usage
        {
            usage.input_tokens = u.input_tokens;
            usage.output_tokens = u.output_tokens;
        }
    }

    let message = last_message.ok_or("Codex exec finished without agent_message output")?;
    Ok((message, usage))
}

#[derive(Deserialize)]
struct ExecEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    item: Option<ExecItem>,
    #[serde(default)]
    usage: Option<ExecUsage>,
}

#[derive(Deserialize)]
struct ExecItem {
    #[serde(rename = "type")]
    item_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ExecUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_retrospective_schema_requires_event_specific_evidence_and_known_states() {
        let schema: serde_json::Value = serde_json::from_str(ANALYSIS_OUTPUT_SCHEMA).unwrap();
        let item = &schema["properties"]["sessions"]["items"];
        let props = &item["properties"];
        assert!(props.get("project").is_none());
        for field in ["task_title", "questions", "follow_ups"] {
            assert!(
                item["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!(field))
            );
        }
        for field in ["questions", "follow_ups"] {
            let record = &props[field]["items"];
            assert_eq!(record["additionalProperties"], false);
            assert!(
                record["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("evidence"))
            );
            let event = &record["properties"]["updates"]["items"];
            assert_eq!(event["additionalProperties"], false);
            assert!(
                event["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("evidence"))
            );
            assert_eq!(event["properties"]["evidence"]["$ref"], "#/$defs/evidence");
        }
        assert_eq!(
            props["questions"]["items"]["properties"]["updates"]["items"]["properties"]["kind"]["enum"],
            serde_json::json!(["answer", "resolved", "open", "unknown"])
        );
        assert_eq!(
            props["follow_ups"]["items"]["properties"]["origin"]["enum"],
            serde_json::json!(["user_request", "ai_suggestion", "unknown"])
        );
        assert_eq!(
            props["follow_ups"]["items"]["properties"]["updates"]["items"]["properties"]["kind"]["enum"],
            serde_json::json!(["accepted", "completed", "withdrawn", "unknown"])
        );
    }

    #[test]
    fn test_analysis_schema_requires_evidence_and_explicit_correction_kind() {
        let schema: serde_json::Value = serde_json::from_str(ANALYSIS_OUTPUT_SCHEMA).unwrap();
        let properties = &schema["properties"]["sessions"]["items"]["properties"];
        for category in ["decisions", "til", "corrections"] {
            let item = &properties[category]["items"];
            assert!(
                item["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("evidence"))
            );
            assert_eq!(item["properties"]["evidence"]["$ref"], "#/$defs/evidence");
            assert_eq!(item["additionalProperties"], false);
        }
        assert_eq!(
            properties["corrections"]["items"]["properties"]["kind"]["enum"],
            serde_json::json!(["factual_error", "preference_change", "unknown"])
        );
        let evidence = &schema["$defs"]["evidence"]["items"];
        assert_eq!(
            evidence["required"],
            serde_json::json!(["message_id", "quote"])
        );
        assert_eq!(evidence["additionalProperties"], false);
        assert!(evidence["properties"].get("location").is_none());
        assert!(evidence["properties"].get("role").is_none());
    }

    #[test]
    fn test_parse_jsonl_events_extracts_agent_message_and_usage() {
        let output = r#"{"type":"thread.started","thread_id":"t1"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"{\"sessions\":[]}"}}
{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":20}}"#;

        let (message, usage) = parse_jsonl_events(output).expect("parse");
        assert_eq!(message, "{\"sessions\":[]}");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
    }

    #[test]
    fn test_parse_jsonl_events_ignores_non_json_lines() {
        let output = r#"not-json
{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"ok"}}
{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":2}}"#;

        let (message, usage) = parse_jsonl_events(output).expect("parse");
        assert_eq!(message, "ok");
        assert_eq!(usage.input_tokens, 1);
        assert_eq!(usage.output_tokens, 2);
    }

    #[test]
    fn test_parse_jsonl_events_missing_agent_message_returns_error() {
        let output = r#"{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":2}}"#;
        let result = parse_jsonl_events(output);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_analysis_schema_file_creates_schema_with_sessions_key() {
        let path = write_analysis_schema_file().expect("schema file should be created");
        let content = std::fs::read_to_string(&path).expect("schema file should be readable");
        assert!(content.contains("\"sessions\""));
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn test_default_model_inherits_codex_cli_configuration() {
        let cmd = build_codex_exec_command(None, 128, crate::config::DEFAULT_CODEX_MODEL, "high");
        let args: Vec<_> = cmd.get_args().map(|arg| arg.to_string_lossy()).collect();
        assert!(!args.iter().any(|arg| arg == "--model"));
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "-s" && pair[1] == "read-only")
        );
    }

    #[test]
    fn test_explicit_model_still_overrides_codex_cli_configuration() {
        let cmd = build_codex_exec_command(None, 128, "custom-model", "high");
        let args: Vec<_> = cmd.get_args().map(|arg| arg.to_string_lossy()).collect();
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "--model" && pair[1] == "custom-model")
        );
    }
}
