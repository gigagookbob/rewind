// OpenAI Chat Completions API client.

use super::planner::RateLimits;
use serde::{Deserialize, Serialize};

const API_URL: &str = "https://api.openai.com/v1/chat/completions";
pub(crate) const MODEL: &str = "gpt-4o";

/// OpenAI Chat Completions request body.
#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

impl ChatRequest {
    fn new(
        system_prompt: &str,
        conversation_text: &str,
        max_tokens: u32,
        json_output: bool,
    ) -> Self {
        Self {
            model: MODEL.to_string(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: conversation_text.to_string(),
                },
            ],
            max_tokens,
            // Chunk summaries and share messages are plain text. JSON mode
            // requires a JSON instruction and rejects their text-only prompts.
            response_format: json_output.then(|| ResponseFormat {
                format_type: "json_object".to_string(),
            }),
        }
    }
}

/// Forces JSON output mode.
#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    format_type: String,
}

/// Chat message (role + content).
#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

/// OpenAI Chat Completions response body.
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<UsageInfo>,
}

/// Token usage from the API response.
#[derive(Deserialize)]
struct UsageInfo {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

/// A single choice from the choices array.
#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

/// The message inside a choice (role is always "assistant").
#[derive(Deserialize)]
struct ChoiceMessage {
    #[serde(default)]
    content: Option<String>,
}

/// Calls the OpenAI Chat Completions API and returns the raw text response.
pub(super) fn build_request(
    api_key: &str,
    system_prompt: &str,
    conversation_text: &str,
    max_tokens: u32,
    json_output: bool,
) -> Result<reqwest::Request, super::AnalyzerError> {
    let client = reqwest::Client::new();

    let request_body = ChatRequest::new(system_prompt, conversation_text, max_tokens, json_output);

    Ok(client
        .post(API_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .build()?)
}

pub(super) async fn send_request(
    request: reqwest::Request,
) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
    let response = reqwest::Client::new().execute(request).await?;

    let status = response.status();
    if !status.is_success() {
        return Err(crate::messages::error::openai_api_request_failed(
            &status,
            "response body omitted",
        )
        .into());
    }

    let chat_response: ChatResponse = response.json().await?;
    decode_response(chat_response)
}

fn decode_response(
    chat_response: ChatResponse,
) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
    let usage = chat_response
        .usage
        .map(|u| super::ApiUsage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        })
        .unwrap_or_default();

    let choice = chat_response
        .choices
        .first()
        .ok_or(crate::messages::error::OPENAI_EMPTY_CHOICES)?;

    if choice.finish_reason.as_deref() == Some("length") {
        return Err(super::OutputTruncated { usage }.into());
    }

    let text = choice
        .message
        .content
        .as_deref()
        .ok_or(crate::messages::error::API_NO_TEXT_BLOCK)?;

    Ok((text.to_string(), usage))
}

#[cfg(test)]
pub(super) fn decode_test_response(
    raw: &str,
) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
    decode_response(serde_json::from_str(raw)?)
}

/// Minimal request body for probing (no response_format needed).
#[derive(Serialize)]
struct ProbeRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: u32,
}

/// Sends a minimal request to probe rate limits from response headers.
pub async fn probe_openai_rate_limits(api_key: &str) -> Option<RateLimits> {
    let client = reqwest::Client::new();

    let request_body = ProbeRequest {
        model: MODEL.to_string(),
        messages: vec![ChatMessage {
            role: "user".to_string(),
            content: "ping".to_string(),
        }],
        max_tokens: 1,
    };

    let response = client
        .post(API_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .ok()?;

    parse_openai_rate_headers(&response)
}

/// Extracts rate limit values from OpenAI response headers.
fn parse_openai_rate_headers(response: &reqwest::Response) -> Option<RateLimits> {
    let headers = response.headers();

    let tpm = headers
        .get("x-ratelimit-limit-tokens")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())?;

    let rpm = headers
        .get("x-ratelimit-limit-requests")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(50);

    Some(RateLimits {
        input_tokens_per_minute: tpm,
        output_tokens_per_minute: tpm / 4,
        requests_per_minute: rpm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies response JSON parsing without making actual API calls.
    #[test]
    fn test_parse_openai_response() {
        let json = r#"{
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4o",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "{\"sessions\": []}"
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 11,
                "total_tokens": 21
            }
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(
            response.choices[0].message.content.as_deref(),
            Some("{\"sessions\": []}")
        );
        assert_eq!(response.choices[0].finish_reason.as_deref(), Some("stop"));
    }

    /// Verifies .first() returns None for an empty choices array.
    #[test]
    fn test_empty_choices() {
        let json = r#"{"choices": []}"#;
        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert!(response.choices.is_empty());
    }
    #[test]
    fn test_text_request_omits_json_response_format() {
        let request = ChatRequest::new("Summarize as plain text", "daily work", 128, false);
        let value = serde_json::to_value(request).expect("serialized request");
        assert!(value.get("response_format").is_none());
        assert_eq!(value["messages"][1]["content"], "daily work");
    }

    #[test]
    fn test_analysis_request_keeps_json_response_format() {
        let request = ChatRequest::new("Return JSON", "daily work", 128, true);
        let value = serde_json::to_value(request).expect("serialized request");
        assert_eq!(value["response_format"]["type"], "json_object");
    }

    #[test]
    fn test_parse_openai_response_allows_null_content() {
        let json = r#"{
            "choices": [
                {
                    "message": { "role": "assistant", "content": null },
                    "finish_reason": "content_filter"
                }
            ]
        }"#;
        let response: ChatResponse = serde_json::from_str(json).expect("null content parses");
        assert_eq!(response.choices.len(), 1);
        let err = decode_test_response(json).unwrap_err().to_string();
        assert_eq!(err, crate::messages::error::API_NO_TEXT_BLOCK);
    }

    #[test]
    fn test_decode_response_finish_reason_length_returns_truncated() {
        let json = r#"{
            "choices": [
                {
                    "message": { "role": "assistant", "content": "{\"sessions\":[" },
                    "finish_reason": "length"
                }
            ],
            "usage": { "prompt_tokens": 12, "completion_tokens": 4096 }
        }"#;
        let err = decode_test_response(json).unwrap_err();
        assert!(
            err.downcast_ref::<super::super::OutputTruncated>()
                .is_some()
        );
        assert!(err.to_string().contains("4096"));
    }
}
