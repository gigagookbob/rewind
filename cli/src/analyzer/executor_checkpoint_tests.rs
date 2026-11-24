use super::super::{session_cache::SessionCache, transport_tests::fake_provider};
use super::*;
use crate::config::Lang;

#[tokio::test]
async fn test_completed_session_is_reused_and_no_cache_forces_new_analysis() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = SessionCache {
        root: root.path().into(),
        reuse: true,
        scope: "day".into(),
    };
    let task =
        redaction_tests::planned(vec![("USER".into(), "hello".into())], StepStrategy::Direct);
    let complete = serde_json::to_string(&tests::sample_analysis("s1")).unwrap();
    let limits = tests::sample_limits(1000, 100_000);
    let (provider, _) = fake_provider(
        provider::ProviderKind::Anthropic,
        true,
        vec![Ok(complete.clone())],
    );
    run_cached_task(task.clone(), &provider, "auth", &limits, &Lang::Ko, &cache)
        .await
        .unwrap();
    let (resumed, requests) = fake_provider(provider::ProviderKind::Anthropic, true, vec![]);
    let saved = run_cached_task(task.clone(), &resumed, "auth", &limits, &Lang::Ko, &cache)
        .await
        .unwrap();
    assert_eq!(saved.analysis.sessions[0].work_summary, "summary-s1");
    assert!(requests.payloads().is_empty());
    cache.reuse = false;
    let (fresh, requests) =
        fake_provider(provider::ProviderKind::Anthropic, true, vec![Ok(complete)]);
    run_cached_task(task, &fresh, "auth", &limits, &Lang::Ko, &cache)
        .await
        .unwrap();
    assert_eq!(requests.payloads().len(), 1);
}

#[tokio::test]
async fn test_failed_session_is_not_checkpointed() {
    let root = tempfile::tempdir().unwrap();
    let cache = SessionCache {
        root: root.path().into(),
        reuse: true,
        scope: "day".into(),
    };
    let task =
        redaction_tests::planned(vec![("USER".into(), "hello".into())], StepStrategy::Direct);
    let (provider, _) = fake_provider(
        provider::ProviderKind::Anthropic,
        true,
        vec![Err("synthetic failure".into())],
    );
    assert!(
        run_cached_task(
            task,
            &provider,
            "auth",
            &tests::sample_limits(1000, 100_000),
            &Lang::Ko,
            &cache
        )
        .await
        .is_err()
    );
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn test_parallel_failure_preserves_other_sessions_and_resume_only_calls_failed_session() {
    let root = tempfile::tempdir().unwrap();
    let cache = || SessionCache {
        root: root.path().into(),
        reuse: true,
        scope: "day".into(),
    };
    let first =
        redaction_tests::planned(vec![("USER".into(), "first".into())], StepStrategy::Direct).task;
    let mut second = first.clone();
    second.session_id = "s2".into();
    let tasks = vec![first, second];
    let (provider, requests) = fake_provider(
        provider::ProviderKind::Anthropic,
        true,
        vec![
            Err("synthetic service failure".into()),
            Ok(serde_json::to_string(&tests::sample_analysis("s2")).unwrap()),
        ],
    );
    let context = super::super::SessionAnalysisContext {
        provider,
        api_key: "auth".into(),
        rate_limits: tests::sample_limits(1000, 100_000),
        cache: cache(),
    };
    let result =
        super::super::analyze_session_tasks(tasks.clone(), false, &Lang::Ko, "test", &context)
            .await;
    assert!(result.is_err());
    assert_eq!(requests.payloads().len(), 2);
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    let (provider, requests) = fake_provider(
        provider::ProviderKind::Anthropic,
        true,
        vec![Ok(
            serde_json::to_string(&tests::sample_analysis("s1")).unwrap()
        )],
    );
    let resumed = super::super::SessionAnalysisContext {
        provider,
        api_key: "auth".into(),
        rate_limits: tests::sample_limits(1000, 100_000),
        cache: cache(),
    };
    let (result, _) =
        super::super::analyze_session_tasks(tasks, false, &Lang::Ko, "test", &resumed)
            .await
            .unwrap();
    assert_eq!(requests.payloads().len(), 1);
    assert_eq!(
        result
            .sessions
            .iter()
            .map(|s| s.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["s1", "s2"]
    );
}

#[tokio::test]
async fn test_checkpoint_write_failure_does_not_discard_completed_analysis() {
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("file");
    std::fs::write(&blocked, "not a directory").unwrap();
    let cache = SessionCache {
        root: blocked,
        reuse: true,
        scope: "day".into(),
    };
    let task =
        redaction_tests::planned(vec![("USER".into(), "hello".into())], StepStrategy::Direct);
    let (provider, _) = fake_provider(
        provider::ProviderKind::Anthropic,
        true,
        vec![Ok(
            serde_json::to_string(&tests::sample_analysis("s1")).unwrap()
        )],
    );
    let result = run_cached_task(
        task,
        &provider,
        "auth",
        &tests::sample_limits(1000, 100_000),
        &Lang::Ko,
        &cache,
    )
    .await
    .unwrap();
    assert_eq!(result.analysis.sessions[0].session_id, "s1");
}
