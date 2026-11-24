use super::super::transport_tests::fake_provider;
use super::*;

fn response(stop: &str, text: &str, output_tokens: u64) -> String {
    serde_json::json!({"stop_reason": stop,
        "content": [{"type": "text", "text": text}],
        "usage": {"input_tokens": 10, "output_tokens": output_tokens}})
    .to_string()
}

#[tokio::test]
async fn test_truncation_retries_with_larger_output_budget_and_sums_usage() {
    let (provider, recorded) = fake_provider(provider::ProviderKind::Anthropic, true, vec![]);
    let complete = serde_json::to_string(&tests::sample_analysis("s1")).unwrap();
    recorded.set_anthropic_responses(vec![
        response("max_tokens", "private partial", 4096),
        response("end_turn", &complete, 5000),
    ]);
    let (result, usage) = analyze_prompt(
        "input".into(),
        &provider,
        "auth",
        &crate::config::Lang::Ko,
        &tests::sample_limits(1000, 100_000),
    )
    .await
    .unwrap();
    assert_eq!(result.sessions[0].session_id, "s1");
    assert_eq!(usage.output_tokens, 9096);
    assert_eq!(usage.input_tokens, 20);
    let payloads = recorded.payloads();
    let budgets: Vec<_> = payloads
        .iter()
        .map(|p| {
            serde_json::from_str::<serde_json::Value>(p).unwrap()["max_tokens"]
                .as_u64()
                .unwrap()
        })
        .collect();
    assert_eq!(budgets, vec![4096, 8192]);
    assert!(payloads.iter().all(|p| !p.contains("private partial")));
}

#[tokio::test]
async fn test_truncation_stops_at_provider_cap_without_json_retry() {
    let (provider, recorded) = fake_provider(provider::ProviderKind::Anthropic, true, vec![]);
    recorded.set_anthropic_responses(
        [4096, 8192, 16384, 32000]
            .into_iter()
            .map(|n| response("max_tokens", "private partial", n))
            .collect(),
    );
    let task =
        redaction_tests::planned(vec![("USER".into(), "hello".into())], StepStrategy::Direct);
    let error = run_planned_task(
        task,
        &provider,
        "auth",
        &tests::sample_limits(1000, 100_000),
        &crate::config::Lang::Ko,
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(error.contains("max_tokens"));
    assert!(error.contains("32000"));
    assert!(!error.contains("private partial"));
    assert_eq!(recorded.payloads().len(), 4);
}
