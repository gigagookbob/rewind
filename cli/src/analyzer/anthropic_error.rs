//! Report actionable provider failures without printing echoed prompts or secrets.

const MAX_ERROR_BYTES: usize = 16 * 1024;

pub(crate) async fn describe(response: reqwest::Response) -> String {
    let status = response.status();
    let request_id = response
        .headers()
        .get("request-id")
        .and_then(|id| id.to_str().ok())
        .filter(|id| {
            id.starts_with("req_")
                && id.len() <= 128
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
        .map(str::to_owned);
    let body = tokio::time::timeout(std::time::Duration::from_secs(5), read_body(response))
        .await
        .ok()
        .flatten();
    let detail = detail(status.as_u16(), body.as_ref());
    let mut message = crate::messages::error::api_request_failed(&status, detail);
    if let Some(id) = request_id {
        message.push_str(&format!(" (request ID: {id})"));
    }
    message
}

async fn read_body(mut response: reqwest::Response) -> Option<serde_json::Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > MAX_ERROR_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).ok()
}

fn detail(status: u16, body: Option<&serde_json::Value>) -> &'static str {
    let error = body.and_then(|body| body.get("error"));
    let message = error
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    // Classify known causes, but never copy provider text into terminal/log output.
    if status == 400 {
        if message.contains("not scoped to a workspace")
            || message.contains("missing 'anthropic-workspace-id' header")
        {
            return "This organization-scoped API key requires a workspace ID. Set it with `rwd config anthropic-workspace-id wrkspc_...` (Console Settings > Workspaces).";
        }
        if message.contains("credit balance is too low") {
            return "Your Anthropic API credit balance is too low. Check billing in the Claude Console.";
        }
        if message.contains("workspace")
            && (message.contains("not found") || message.contains("does not exist"))
        {
            return "The selected Anthropic workspace was not found. Check `rwd config anthropic-workspace-id` against Console Settings > Workspaces.";
        }
    }
    match status {
        401 => "Anthropic authentication failed. Check that the API key is active and correct.",
        403 => {
            "Anthropic denied access. Check the key's permissions and access to the selected workspace or model."
        }
        404 => {
            "The requested Anthropic resource was not found. Check the configured workspace and model availability."
        }
        413 => "The Anthropic request is too large. Reduce the input size.",
        429 => {
            "Anthropic rate limit exceeded. Wait before retrying or check your workspace limits."
        }
        529 => "Anthropic is temporarily overloaded. Retry later.",
        500..=599 => "Anthropic returned a server error. Retry later.",
        _ => match error
            .and_then(|e| e.get("type"))
            .and_then(serde_json::Value::as_str)
        {
            Some("invalid_request_error") => {
                "Anthropic rejected the request (invalid_request_error). Unrecognized details are omitted to protect credentials and conversation text."
            }
            Some("billing_error") => {
                "Anthropic reported a billing error. Check billing in the Claude Console."
            }
            _ => {
                "No safe error details are available. Provider response body omitted to protect credentials and conversation text."
            }
        },
    }
}
