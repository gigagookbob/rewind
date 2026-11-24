// Anthropic Claude Messages API client.

use super::planner::RateLimits;
use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
pub(super) const MODEL: &str = "claude-opus-4-6";

#[cfg(test)]
#[path = "anthropic_error_tests.rs"]
mod error_tests;

/// Claude Messages API request body.
#[derive(Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<OutputConfig>,
}

/// Anthropic Structured Outputs configuration (`output_config.format`).
#[derive(Serialize)]
struct OutputConfig {
    format: OutputFormat,
}

#[derive(Serialize)]
struct OutputFormat {
    #[serde(rename = "type")]
    format_type: &'static str,
    schema: serde_json::Value,
}

/// JSON Schema enforced via Anthropic constrained decoding for session analysis.
///
/// Shares `evidence` through `$defs` and omits deprecated legacy fields
/// (`curiosities`, `follow_ups`) to stay within Anthropic's grammar size limit.
pub(super) fn analysis_output_schema() -> serde_json::Value {
    let evidence_ref = serde_json::json!({"type": "array", "items": {"$ref": "#/$defs/evidence"}});
    serde_json::json!({
        "type": "object",
        "$defs": {
            "evidence": {
                "type": "object",
                "properties": {"message_id": {"type": "string"}, "quote": {"type": "string"}},
                "required": ["message_id", "quote"],
                "additionalProperties": false
            }
        },
        "properties": {
            "sessions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "session_id": {"type": "string"},
                        "work_summary": {"type": "string"},
                        "task_title": {
                            "anyOf": [
                                {
                                    "type": "object",
                                    "properties": {"text": {"type": "string"}, "evidence": evidence_ref},
                                    "required": ["text", "evidence"],
                                    "additionalProperties": false
                                },
                                {"type": "null"}
                            ]
                        },
                        "questions": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "question": {"type": "string"},
                                    "evidence": evidence_ref,
                                    "updates": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "kind": {"type": "string", "enum": ["answer", "resolved", "open", "unknown"]},
                                                "detail": {"type": "string"},
                                                "evidence": evidence_ref
                                            },
                                            "required": ["kind", "detail", "evidence"],
                                            "additionalProperties": false
                                        }
                                    }
                                },
                                "required": ["question", "evidence", "updates"],
                                "additionalProperties": false
                            }
                        },
                        "decisions": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {"what": {"type": "string"}, "why": {"type": "string"}, "evidence": evidence_ref},
                                "required": ["what", "why", "evidence"],
                                "additionalProperties": false
                            }
                        },
                        "corrections": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "model_said": {"type": "string"},
                                    "user_corrected": {"type": "string"},
                                    "kind": {"type": "string", "enum": ["factual_error", "preference_change", "unknown"]},
                                    "evidence": evidence_ref
                                },
                                "required": ["model_said", "user_corrected", "kind", "evidence"],
                                "additionalProperties": false
                            }
                        },
                        "til": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {"title": {"type": "string"}, "detail": {"type": "string"}, "evidence": evidence_ref},
                                "required": ["title", "detail", "evidence"],
                                "additionalProperties": false
                            }
                        }
                    },
                    "required": ["session_id", "work_summary", "task_title", "questions", "decisions", "corrections", "til"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["sessions"],
        "additionalProperties": false
    })
}

/// API message (role + content).
#[derive(Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

/// Claude Messages API response body.
#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ApiContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<UsageInfo>,
}

/// Token usage from the API response.
#[derive(Deserialize)]
struct UsageInfo {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

/// A content block in the response.
#[derive(Deserialize)]
struct ApiContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
}

/// Calls the Anthropic Claude Messages API and returns the raw text response.
pub(super) fn build_request(
    api_key: &str,
    workspace_id: Option<&str>,
    system_prompt: &str,
    conversation_text: &str,
    max_tokens: u32,
    json_output: bool,
) -> Result<reqwest::Request, super::AnalyzerError> {
    let client = reqwest::Client::new();

    let request_body = ApiRequest {
        model: MODEL.to_string(),
        max_tokens,
        system: system_prompt.to_string(),
        messages: vec![ApiMessage {
            role: "user".to_string(),
            content: conversation_text.to_string(),
        }],
        output_config: json_output.then(|| OutputConfig {
            format: OutputFormat {
                format_type: "json_schema",
                schema: analysis_output_schema(),
            },
        }),
    };

    Ok(with_workspace(
        client
            .post(API_URL)
            .header("x-api-key", api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json"),
        workspace_id,
    )?
    .json(&request_body)
    .build()?)
}

/// Apply the same workspace selection to inference, probes, and credential checks.
pub(crate) fn with_workspace(
    request: reqwest::RequestBuilder,
    workspace_id: Option<&str>,
) -> Result<reqwest::RequestBuilder, super::AnalyzerError> {
    match crate::config::parse_anthropic_workspace_id(workspace_id.unwrap_or(""))? {
        Some(id) => Ok(request.header("anthropic-workspace-id", id)),
        None => Ok(request),
    }
}

pub(super) async fn send_request(
    request: reqwest::Request,
) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
    let response = reqwest::Client::new().execute(request).await?;

    let status = response.status();
    if !status.is_success() {
        return Err(super::anthropic_error::describe(response).await.into());
    }

    let api_response: ApiResponse = response.json().await?;
    decode_response(api_response)
}

fn decode_response(
    api_response: ApiResponse,
) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
    let usage = api_response
        .usage
        .map(|u| super::ApiUsage {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
        })
        .unwrap_or_default();

    if api_response.stop_reason.as_deref() == Some("max_tokens") {
        return Err(super::OutputTruncated { usage }.into());
    }

    let text = api_response
        .content
        .iter()
        .find(|block| block.block_type == "text")
        .and_then(|block| block.text.as_deref())
        .ok_or(crate::messages::error::API_NO_TEXT_BLOCK)?;

    Ok((text.to_string(), usage))
}

#[cfg(test)]
pub(super) fn decode_test_response(
    raw: &str,
) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
    decode_response(serde_json::from_str(raw)?)
}

/// Sends a minimal request to probe rate limits from response headers.
/// Network/transient failures use defaults; rejected credentials/configuration stop analysis.
pub async fn probe_anthropic_rate_limits(
    api_key: &str,
    workspace_id: Option<&str>,
) -> Result<Option<RateLimits>, super::AnalyzerError> {
    let request = build_request(api_key, workspace_id, "", "ping", 1, false)?;
    let response = reqwest::Client::new().execute(request).await;
    match response {
        Ok(response) => rate_limits_from_response(response).await,
        Err(_) => Ok(None),
    }
}

async fn rate_limits_from_response(
    response: reqwest::Response,
) -> Result<Option<RateLimits>, super::AnalyzerError> {
    let status = response.status();
    if status.is_client_error() && status != reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(super::anthropic_error::describe(response).await.into());
    }
    if !status.is_success() {
        return Ok(None);
    }
    Ok(parse_anthropic_rate_headers(&response))
}

/// Extracts rate limit values from Anthropic response headers.
fn parse_anthropic_rate_headers(response: &reqwest::Response) -> Option<RateLimits> {
    let headers = response.headers();

    let itpm = headers
        .get("anthropic-ratelimit-input-tokens-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())?;

    let otpm = headers
        .get("anthropic-ratelimit-output-tokens-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(itpm / 4);

    let rpm = headers
        .get("anthropic-ratelimit-requests-limit")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(50);

    Some(RateLimits {
        input_tokens_per_minute: itpm,
        output_tokens_per_minute: otpm,
        requests_per_minute: rpm,
    })
}
