use super::tests::{sample_analysis, sample_limits};
use super::*;

#[tokio::test]
async fn test_subagent_lossy_caps_defer_recognizable_secrets_until_common_policy() {
    use super::super::transport_tests::fake_provider;
    use crate::sources::subagent::{SubagentRun, subagent_lines};
    for enabled in [true, false] {
        let secret = format!("sk-proj-{}", "a".repeat(10_000));
        let runs = vec![SubagentRun {
            name: "synthetic".into(),
            task: secret.clone(),
            result: secret.clone(),
            at: "2026-11-02T00:00:00Z".parse().unwrap(),
        }];
        let messages = subagent_lines(&runs)
            .into_iter()
            .map(|line| (line.role.into(), line.text))
            .collect();
        let mut task = planned(messages, StepStrategy::Direct);
        task.task.evidence_sources.clear();
        let (provider, recorded) = fake_provider(
            provider::ProviderKind::OpenAi,
            enabled,
            vec![Ok(serde_json::to_string(&sample_analysis("s1")).unwrap())],
        );
        run_planned_task(
            task,
            &provider,
            "auth",
            &sample_limits(1_000, 100_000),
            &crate::config::Lang::Ko,
        )
        .await
        .unwrap();
        let payload = &recorded.payloads()[0];
        assert_eq!(payload.contains(&"a".repeat(100)), !enabled);
        assert!(payload.contains("SUBAGENT_RESULT"));
        assert!(!payload.contains("omitted"));
    }
}

#[tokio::test]
async fn test_antigravity_adapter_retains_detectable_pem_until_real_request_preparation() {
    use super::super::transport_tests::fake_provider;
    use crate::sources::antigravity::AntigravityAdapter;
    use crate::sources::{CollectContext, SourceAdapter};
    use chrono::TimeZone;
    let root = std::env::temp_dir().join(format!("rwd-redaction-agy-{}", std::process::id()));
    let logs = root.join("s1/.system_generated/logs");
    std::fs::create_dir_all(&logs).unwrap();
    let at = chrono::Local
        .with_ymd_and_hms(2020, 1, 1, 12, 0, 0)
        .single()
        .unwrap();
    let log_text = (0..30)
        .map(|i| format!("2020-01-01T12:00:00 {} {i}\n", "L".repeat(300)))
        .collect::<String>();
    let body = (0..30)
        .map(|i| format!("SYNTHETIC_PRIVATE_{i}_{}\n", "K".repeat(300)))
        .collect::<String>();
    let raw = format!(
        "{log_text}-----BEGIN RSA PRIVATE KEY-----\n{body}-----END RSA PRIVATE KEY-----\n마지막 질문입니다."
    );
    let entry = serde_json::json!({"type":"USER_INPUT", "step_index":1,
        "created_at":at.to_rfc3339(), "content":format!("<USER_REQUEST>{raw}</USER_REQUEST>")});
    std::fs::write(logs.join("transcript_full.jsonl"), entry.to_string()).unwrap();
    let ctx = CollectContext {
        date: at.date_naive(),
        config: None,
        include_automated: false,
    };
    let mut collection = AntigravityAdapter::with_roots(vec![root.clone()])
        .collect(&ctx)
        .unwrap();
    assert_eq!(collection.sessions.len(), 1);
    let source_task = collection.sessions.remove(0).task;
    let mut task = planned(vec![], StepStrategy::Direct);
    task.task = source_task;
    let (provider, recorded) = fake_provider(
        provider::ProviderKind::Anthropic,
        true,
        vec![Ok(serde_json::to_string(&sample_analysis("s1")).unwrap())],
    );
    run_planned_task(
        task,
        &provider,
        "auth",
        &sample_limits(1000, 100_000),
        &crate::config::Lang::Ko,
    )
    .await
    .unwrap();
    assert!(!recorded.payloads()[0].contains("SYNTHETIC_PRIVATE_"));
    assert!(recorded.payloads()[0].contains("마지막 질문입니다"));
    std::fs::remove_dir_all(root).unwrap();
}

fn planned(messages: Vec<(String, String)>, strategy: StepStrategy) -> PlannedSessionTask {
    let sources = messages
        .iter()
        .enumerate()
        .map(|(i, (role, text))| super::super::evidence::SourceMessage {
            timestamp: None,
            is_reference: false,
            message_index: i + 1,
            role: role.clone(),
            text: text.clone(),
            rendered_text: text.clone(),
            location: crate::parser::SourceLocation {
                path: "/synthetic/session.jsonl".into(),
                line: i + 10,
            },
        })
        .collect();
    PlannedSessionTask {
        index: 0,
        step_number: 1,
        total_steps: 1,
        estimated_tokens: 100,
        strategy,
        task: SessionTask {
            source: "synthetic",
            session_id: "s1".into(),
            prompt_text: String::new(),
            messages,
            evidence_sources: sources,
            estimated_tokens: 100,
        },
    }
}

#[tokio::test]
async fn test_direct_and_retries_mask_actual_requests_and_count_original_once() {
    use super::super::transport_tests::fake_provider;
    let secret = format!("sk-proj-{}", "a".repeat(80));
    for kind in [
        provider::ProviderKind::Anthropic,
        provider::ProviderKind::OpenAi,
        provider::ProviderKind::Codex {
            model: "fake".into(),
            reasoning_effort: "low".into(),
        },
    ] {
        for enabled in [true, false] {
            for first in [
                Ok("invalid JSON".into()),
                Err("HTTP 429 Too Many Requests".into()),
            ] {
                let response = serde_json::to_string(&sample_analysis("s1")).unwrap();
                let (provider, recorded) =
                    fake_provider(kind.clone(), enabled, vec![first, Ok(response)]);
                let task = planned(vec![("USER".into(), secret.clone())], StepStrategy::Direct);
                let result = run_planned_task(
                    task,
                    &provider,
                    "synthetic-auth",
                    &sample_limits(1000, 100_000),
                    &crate::config::Lang::Ko,
                )
                .await
                .unwrap();
                assert_eq!(result.redact.total_count, usize::from(enabled));
                let payloads = recorded.payloads();
                assert_eq!(payloads.len(), 2);
                for payload in payloads {
                    assert_eq!(payload.contains(&secret), !enabled);
                }
            }
        }
    }
}

#[tokio::test]
async fn test_long_session_masks_before_splitting_and_rechecks_derived_final_input() {
    use super::super::transport_tests::fake_provider;
    let tail = "SYNTHETIC_KEY_TAIL".repeat(200);
    let key = format!("sk-proj-{}{tail}", "a".repeat(80));
    let derived = format!("sk-ant-{}", "b".repeat(60));
    let task = planned(
        vec![
            ("USER".into(), key[..30].into()),
            ("USER".into(), key[30..].into()),
            ("ASSISTANT".into(), "중간🙂".repeat(9_000)),
            ("USER".into(), "마지막에 B로 변경했습니다".into()),
        ],
        StepStrategy::Summarize { chunks: 4 },
    );
    let responses = vec![
        Ok(format!("first {derived}")),
        Ok("penultimate".into()),
        Ok("last [m4] 마지막에 B로 변경했습니다".into()),
        Ok(serde_json::to_string(&sample_analysis("s1")).unwrap()),
    ];
    let (provider, recorded) = fake_provider(provider::ProviderKind::OpenAi, true, responses);
    let result = run_planned_task(
        task,
        &provider,
        "auth",
        &sample_limits(1_000, 2_000),
        &crate::config::Lang::Ko,
    )
    .await
    .unwrap();
    let payloads = recorded.payloads();
    assert_eq!(payloads.len(), 4);
    for payload in &payloads {
        assert!(!payload.contains("SYNTHETIC_KEY_TAIL"));
        assert!(!payload.contains(&derived));
        let value: serde_json::Value = serde_json::from_str(payload).unwrap();
        let system = value["messages"][0]["content"].as_str().unwrap();
        let text = value["messages"][1]["content"].as_str().unwrap();
        assert!(super::super::prompt::estimate_tokens(&format!("{system}\n{text}")) <= 2_000);
    }
    assert_eq!(result.redact.total_count, 1);
    assert!(payloads[0].contains("[REDACTED:API_KEY]"));
    assert!(payloads[2].contains("마지막에 B로 변경했습니다"));
    assert!(payloads[3].contains("[REDACTED:API_KEY]"));
    assert!(result.analysis.sessions[0].work_summary.contains("부분"));
}

#[tokio::test]
async fn test_quotes_require_both_original_and_protected_speech_and_cache_uses_same_policy() {
    use super::super::transport_tests::fake_provider;
    let secret = format!("sk-proj-{}", "a".repeat(80));
    let task = planned(
        vec![
            ("USER".into(), format!("B를 사용합니다 {secret}")),
            ("USER".into(), "-----BEGIN RSA PRIVATE KEY-----".into()),
            ("USER".into(), "SYNTHETIC_PRIVATE_BODY".into()),
            ("USER".into(), "-----END RSA PRIVATE KEY-----".into()),
        ],
        StepStrategy::Direct,
    );
    let decisions = [
            ("m1", "B를 사용합니다"), ("m1", secret.as_str()),
            ("m1", "[REDACTED:API_KEY]"), ("m3", "SYNTHETIC_PRIVATE_BODY"),
        ].map(|(id, quote)| serde_json::json!({"what":"choice", "why":"reason", "evidence":[{"message_id":id,"quote":quote}]}));
    let json = serde_json::json!({"sessions":[{"session_id":"s1", "work_summary":"summary", "decisions":decisions, "curiosities":[], "corrections":[]}]}).to_string();
    let (provider, recorded) = fake_provider(
        provider::ProviderKind::Anthropic,
        true,
        vec![Ok(json.clone())],
    );
    let cached_task = task.task.clone();
    let result = run_planned_task(
        task,
        &provider,
        "auth",
        &sample_limits(1000, 100_000),
        &crate::config::Lang::Ko,
    )
    .await
    .unwrap();
    let decisions = &result.analysis.sessions[0].decisions;
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].evidence[0].location.as_ref().unwrap().line, 10);
    assert_eq!(decisions[0].evidence[0].quote, "B를 사용합니다");
    assert!(!recorded.payloads()[0].contains("SYNTHETIC_PRIVATE_BODY"));
    // Populate old attribution without masking, then revalidate as current code does.
    let (_, original, _) = super::super::evidence::annotate_protected_messages(
        &cached_task.messages,
        &cached_task.evidence_sources,
        false,
    );
    let mut cached: AnalysisResult = serde_json::from_str(&json).unwrap();
    super::super::evidence::validate(&mut cached, "s1", &original);
    assert_eq!(cached.sessions[0].decisions.len(), 3);
    super::super::evidence::revalidate_cached_session(
        &mut cached.sessions[0],
        &cached_task.evidence_sources,
        &cached_task.messages,
        true,
    );
    assert!(
        cached.sessions[0].decisions[0].evidence[0]
            .location
            .is_some()
    );
    assert!(
        cached.sessions[0].decisions[1].evidence[0]
            .location
            .is_none()
    );
    assert!(
        cached.sessions[0].decisions[2].evidence[0]
            .location
            .is_none()
    );
}

#[tokio::test]
async fn test_codex_process_rate_error_is_safe_and_retried() {
    use super::super::transport_tests::fake_provider;
    use std::os::unix::process::ExitStatusExt;
    let secret = format!("sk-proj-{}", "c".repeat(40));
    for stdout in [
        serde_json::json!({"type":"turn.failed","error":{"message":format!("HTTP 429 {secret}")}})
            .to_string(),
        serde_json::json!({"type":"error","message":format!("rate limit exceeded {secret}")})
            .to_string(),
    ] {
        let output = std::process::Output {
            status: std::process::ExitStatus::from_raw(256),
            stdout: stdout.into_bytes(),
            stderr: secret.as_bytes().to_vec(),
        };
        let error = super::super::codex_exec::parse_output(output)
            .unwrap_err()
            .to_string();
        assert!(!error.contains(&secret));
        assert_eq!(classify_retry_action(&error), RetryAction::RateLimit429);
        let (provider, recorded) = fake_provider(
            provider::ProviderKind::Codex {
                model: "fake".into(),
                reasoning_effort: "low".into(),
            },
            true,
            vec![
                Err(error),
                Ok(serde_json::to_string(&sample_analysis("s1")).unwrap()),
            ],
        );
        let task = planned(vec![("USER".into(), secret.clone())], StepStrategy::Direct);
        run_planned_task(
            task,
            &provider,
            "",
            &sample_limits(1000, 100_000),
            &crate::config::Lang::En,
        )
        .await
        .unwrap();
        assert_eq!(recorded.payloads().len(), 2);
        assert!(
            recorded
                .payloads()
                .iter()
                .all(|text| !text.contains(&secret))
        );
    }
    let output = std::process::Output {
        status: std::process::ExitStatus::from_raw(256),
        stdout: secret.as_bytes().to_vec(),
        stderr: secret.as_bytes().to_vec(),
    };
    let error = super::super::codex_exec::parse_output(output)
        .unwrap_err()
        .to_string();
    assert!(!error.contains(&secret));
    assert_eq!(classify_retry_action(&error), RetryAction::None);
    let json_error = insight::parse_response(&format!("{{\"sessions\":\"{secret}\"}}"))
        .unwrap_err()
        .to_string();
    assert!(!json_error.contains(&secret));
    assert_eq!(classify_retry_action(&json_error), RetryAction::JsonParse);
}

#[tokio::test]
async fn test_reference_secret_is_masked_before_bounded_tail_selection() {
    use super::super::transport_tests::fake_provider;
    let secret = format!("sk-proj-{}", "a".repeat(6_000));
    let reference = format!(
        "[REFERENCE 2026-11-01T00:00:00+00:00; prior context only] {}",
        "a".repeat(3_900)
    );
    for enabled in [true, false] {
        let mut task = planned(
            vec![
                ("USER".into(), reference.clone()),
                ("USER".into(), "오늘 B를 적용합니다".into()),
            ],
            StepStrategy::Direct,
        );
        task.task.evidence_sources[0].text = secret.clone();
        task.task.evidence_sources[0].is_reference = true;
        let (provider, recorded) = fake_provider(
            provider::ProviderKind::OpenAi,
            enabled,
            vec![Ok(serde_json::to_string(&sample_analysis("s1")).unwrap())],
        );
        run_planned_task(
            task,
            &provider,
            "auth",
            &sample_limits(1_000, 100_000),
            &crate::config::Lang::Ko,
        )
        .await
        .unwrap();
        let payload = &recorded.payloads()[0];
        assert_eq!(payload.contains(&"a".repeat(100)), !enabled);
        assert!(payload.contains("[REFERENCE 2026-11-01"));
        assert!(payload.contains("오늘 B를 적용합니다"));
    }
}

#[tokio::test]
async fn test_today_compaction_does_not_hide_private_key_header_from_masking() {
    use super::super::transport_tests::fake_provider;
    let logs = (0..30)
        .map(|i| format!("2026-11-02T12:00:00 {} {i}\n", "L".repeat(300)))
        .collect::<String>();
    let body = (0..30)
        .map(|i| format!("SYNTHETIC_PRIVATE_{i}_{}\n", "K".repeat(300)))
        .collect::<String>();
    let raw = format!(
        "{logs}-----BEGIN RSA PRIVATE KEY-----\n{body}-----END RSA PRIVATE KEY-----\n이후 B를 사용합니다."
    );
    let rendered = super::super::compaction::compact_log_like(&raw);
    let mut task = planned(vec![("USER".into(), rendered)], StepStrategy::Direct);
    task.task.evidence_sources[0].text = raw;
    let (provider, recorded) = fake_provider(
        provider::ProviderKind::OpenAi,
        true,
        vec![Ok(serde_json::to_string(&sample_analysis("s1")).unwrap())],
    );
    run_planned_task(
        task,
        &provider,
        "auth",
        &sample_limits(1000, 100_000),
        &crate::config::Lang::Ko,
    )
    .await
    .unwrap();
    assert!(!recorded.payloads()[0].contains("SYNTHETIC_PRIVATE_"));
    assert!(recorded.payloads()[0].contains("이후 B를 사용합니다"));
}
