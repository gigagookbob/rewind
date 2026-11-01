//! Fake transport receives the same built HTTP requests and Codex stdin as production.

use super::provider::{LlmProvider, OutboundRequest, ProviderKind};
use super::{AnalyzerError, ApiUsage};
use crate::config::Lang;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct Recording(Arc<Mutex<Recorded>>);

#[derive(Default)]
struct Recorded {
    requests: Vec<OutboundRequest>,
    responses: VecDeque<Result<String, String>>,
}

impl Recording {
    pub(super) fn send(
        &self,
        request: OutboundRequest,
    ) -> Result<(String, ApiUsage), AnalyzerError> {
        let mut state = self.0.lock().unwrap();
        state.requests.push(request);
        state
            .responses
            .pop_front()
            .expect("fake response required")
            .map(|text| (text, ApiUsage::default()))
            .map_err(Into::into)
    }

    pub(super) fn payloads(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .requests
            .iter()
            .map(|request| match request {
                OutboundRequest::Anthropic(r) | OutboundRequest::OpenAi(r) => {
                    String::from_utf8(r.body().unwrap().as_bytes().unwrap().to_vec()).unwrap()
                }
                OutboundRequest::Codex { stdin, .. } => stdin.clone(),
            })
            .collect()
    }
}

pub(super) fn fake_provider(
    kind: ProviderKind,
    enabled: bool,
    responses: Vec<Result<String, String>>,
) -> (LlmProvider, Recording) {
    let recording = Recording::default();
    recording.0.lock().unwrap().responses = responses.into();
    let mut provider = LlmProvider::new(kind, enabled);
    provider.recording = Some(recording.clone());
    (provider, recording)
}

fn kinds() -> Vec<ProviderKind> {
    vec![
        ProviderKind::Anthropic,
        ProviderKind::OpenAi,
        ProviderKind::Codex {
            model: "fake".into(),
            reasoning_effort: "low".into(),
        },
    ]
}

#[tokio::test]
async fn test_explicit_disable_keeps_all_input_and_system_prompt_is_also_protected() {
    let secret = format!("sk-ant-{}", "b".repeat(40));
    for kind in kinds() {
        for enabled in [true, false] {
            let (provider, recorded) = fake_provider(kind.clone(), enabled, vec![Ok("ok".into())]);
            provider
                .call_text_api("synthetic-auth", &secret, &secret, 1, 10_000)
                .await
                .unwrap();
            let payload = &recorded.payloads()[0];
            assert_eq!(payload.contains(&secret), !enabled);
            assert_eq!(
                payload.matches("[REDACTED:API_KEY]").count(),
                if enabled { 2 } else { 0 }
            );
        }
    }
}

#[tokio::test]
async fn test_expansion_is_checked_before_transport_without_truncation() {
    let text = "xoxb-a ".repeat(100);
    for kind in kinds() {
        let budget =
            super::prompt::estimate_tokens(&super::codex_exec::compose_prompt("system", &text));
        let (provider, recorded) = fake_provider(kind.clone(), true, vec![]);
        let error = provider
            .call_text_api("auth", "system", &text, 1, budget)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no conversation text was truncated")
        );
        assert!(recorded.payloads().is_empty());
        let (provider, recorded) = fake_provider(kind, false, vec![Ok("ok".into())]);
        provider
            .call_text_api("auth", "system", &text, 1, budget)
            .await
            .unwrap();
        assert!(recorded.payloads()[0].contains(&text));
    }
}

#[test]
fn test_missing_invalid_or_unreadable_config_never_disables_protection() {
    let root = std::env::temp_dir().join(format!("rwd-redaction-config-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("config.toml");
    assert!(super::provider::load_provider_from_path(&root.join("missing")).is_err());
    assert!(super::provider::load_provider_from_path(&root).is_err());
    let base = "[llm]\nprovider = 'codex'\n[output]\npath = '/synthetic'\n";
    for (extra, expected) in [
        ("", true),
        ("[redactor]\nenabled = true", true),
        ("[redactor]\nenabled = false", false),
    ] {
        std::fs::write(&path, format!("{base}{extra}")).unwrap();
        let (provider, _) = super::provider::load_provider_from_path(&path).unwrap();
        assert_eq!(provider.redactor_enabled(), expected);
    }
    let malformed = format!("{base}[redactor]\nenabled = 'sk-proj-{}'", "a".repeat(40));
    std::fs::write(&path, &malformed).unwrap();
    let error = super::provider::load_provider_from_path(&path)
        .err()
        .unwrap()
        .to_string();
    assert!(!error.contains("sk-proj-"));
    let invalid_provider = format!(
        "[llm]\nprovider = 'sk-proj-{}'\n[output]\npath = '/synthetic'",
        "a".repeat(40)
    );
    std::fs::write(&path, invalid_provider).unwrap();
    assert!(
        !super::provider::load_provider_from_path(&path)
            .err()
            .unwrap()
            .to_string()
            .contains("sk-proj-")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn test_codex_checks_full_stdin_and_system_prompt_even_with_empty_conversation() {
    let kind = kinds().pop().unwrap();
    let (provider, recorded) = fake_provider(kind, true, vec![Ok("ok".into())]);
    let cap = provider
        .max_conversation_chars_with_prompt("system")
        .unwrap();
    provider
        .call_text_api("", "system", &"가".repeat(cap), 1, 1_000_000)
        .await
        .unwrap();
    assert!(
        provider
            .call_text_api("", "system", &"가".repeat(cap + 1), 1, 1_000_000)
            .await
            .is_err()
    );
    assert!(
        provider
            .call_text_api("", &"가".repeat(1_048_576), "", 1, 1_000_000)
            .await
            .is_err()
    );
    assert_eq!(recorded.payloads().len(), 1);
    assert_eq!(recorded.payloads()[0].chars().count() + 8_192, 1_048_576);
}

#[tokio::test]
async fn test_budget_counts_real_prompt_wrappers_and_content_after_masking() {
    for kind in kinds() {
        let system = "system instruction";
        let input = "[Source chunk 123/456; chronological order]\n[USER] xoxb-a";
        let (provider, recorded) = fake_provider(kind, true, vec![Ok("ok".into())]);
        let masked = crate::redactor::redact_text(input).0;
        let exact = provider.estimate_input_tokens(system, &masked);
        assert!(
            provider
                .call_text_api("auth", system, input, 1, exact - 1)
                .await
                .is_err()
        );
        provider
            .call_text_api("auth", system, input, 1, exact)
            .await
            .unwrap();
        assert_eq!(recorded.payloads().len(), 1);
        assert!(recorded.payloads()[0].contains("[REDACTED:SLACK_TOKEN]"));
    }
}

#[tokio::test]
async fn test_all_provider_payloads_mask_cache_summaries_and_keep_authentication() {
    let secret = format!("sk-proj-{}", "a".repeat(40));
    for kind in kinds() {
        let (provider, recorded) =
            fake_provider(kind, true, vec![Ok("summary".into()), Ok("slack".into())]);
        provider
            .call_summary_api(&secret, &format!("old cache {secret}"), &Lang::Ko)
            .await
            .unwrap();
        provider
            .call_slack_api(&secret, &format!("old cache {secret}"), &Lang::En)
            .await
            .unwrap();
        let payloads = recorded.payloads();
        assert_eq!(payloads.len(), 2);
        for payload in payloads {
            assert!(!payload.contains(&secret));
            assert!(payload.contains("[REDACTED:API_KEY]"));
        }
        for request in &recorded.0.lock().unwrap().requests {
            match request {
                OutboundRequest::Anthropic(r) => assert_eq!(r.headers()["x-api-key"], secret),
                OutboundRequest::OpenAi(r) => {
                    assert_eq!(r.headers()["authorization"], format!("Bearer {secret}"))
                }
                OutboundRequest::Codex {
                    stdin, json_output, ..
                } => {
                    assert!(stdin.starts_with("[System Instructions]\n"));
                    assert!(!json_output);
                }
            }
        }
    }
}
