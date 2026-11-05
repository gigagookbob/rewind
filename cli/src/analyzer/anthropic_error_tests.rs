use std::io::{Read, Write};

fn serve(status: u16, body: &str) -> (reqwest::Request, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let body = body.to_string();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
            assert!(request.len() <= 4096);
        }
        write!(stream, "HTTP/1.1 {status} Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\nrequest-id: req_test123\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let request = reqwest::Client::new()
        .get(format!("http://{address}"))
        .build()
        .unwrap();
    (request, server)
}

async fn failure(status: u16, body: &str) -> String {
    let (request, server) = serve(status, body);
    let error = super::send_request(request).await.unwrap_err().to_string();
    server.join().unwrap();
    error
}

#[tokio::test]
async fn test_probe_stops_on_configuration_errors_but_keeps_transient_fallback() {
    for status in [400, 401, 403, 404, 429, 500, 529, 200] {
        let (request, server) = serve(
            status,
            r#"{"error":{"type":"invalid_request_error","message":"This API key is not scoped to a workspace"}}"#,
        );
        let response = reqwest::Client::new().execute(request).await.unwrap();
        let result = super::rate_limits_from_response(response).await;
        server.join().unwrap();
        assert_eq!(result.is_err(), matches!(status, 400 | 401 | 403 | 404));
        if status == 400 {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("rwd config anthropic-workspace-id")
            );
        }
    }
}

#[tokio::test]
async fn test_auth_permission_and_rate_limit_errors_are_distinct() {
    for (status, expected) in [
        (401, "authentication failed"),
        (403, "denied access"),
        (429, "rate limit exceeded"),
        (529, "overloaded"),
    ] {
        assert!(failure(status, "{}").await.contains(expected));
    }
}

#[tokio::test]
async fn test_missing_workspace_explains_how_to_configure_it() {
    let error = failure(400, r#"{"error":{"type":"invalid_request_error","message":"This API key is not scoped to a workspace, so this request must include the anthropic-workspace-id header with the ID of the workspace to use."}}"#).await;
    assert!(
        error.contains("rwd config anthropic-workspace-id"),
        "{error}"
    );
    assert!(error.contains("400"));
    assert!(error.contains("req_test123"));
    assert!(!error.contains("key is invalid"));
}

#[tokio::test]
async fn test_credit_failure_is_not_reported_as_an_invalid_key() {
    let error = failure(400, r#"{"error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API."}}"#).await;
    assert!(error.contains("credit balance"), "{error}");
    assert!(!error.contains("key is invalid"));
}

#[tokio::test]
async fn test_provider_echoes_and_malformed_responses_stay_private() {
    for body in [
        r#"{"error":{"type":"invalid_request_error","message":"Private conversation: jane@example.com sk-ant-secret password=hidden"}}"#.to_string(),
        r#"{"error":{"type":"sk-ant-secret","message":"password=hidden"},"request_id":"sk-ant-secret"}"#.to_string(),
        "<html>jane@example.com password=hidden</html>".to_string(),
        format!(r#"{{"error":{{"message":"{}password=hidden"}}}}"#, "x".repeat(20_000)),
    ] {
        let error = failure(400, &body).await;
        for secret in ["jane@example.com", "sk-ant-secret", "password=hidden", "Private conversation"] {
            assert!(!error.contains(secret), "{error}");
        }
        assert!(error.contains("400"));
    }
}
