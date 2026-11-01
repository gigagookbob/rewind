//! Exercise the common execution boundary, with all actual outbound builders.
use super::super::{retrospective, retrospective_tests as fixture, transport_tests::fake_provider};
use super::*;

#[tokio::test]
async fn test_retrospective_all_providers_languages_and_summary_paths_validate_original_speech() {
    for kind in [
        provider::ProviderKind::Anthropic,
        provider::ProviderKind::OpenAi,
        provider::ProviderKind::Codex {
            model: "fake".into(),
            reasoning_effort: "low".into(),
        },
    ] {
        for lang in [crate::config::Lang::Ko, crate::config::Lang::En] {
            for summarize in [false, true] {
                let sources = fixture::sources();
                let messages = sources
                    .iter()
                    .map(|s| (s.role.clone(), s.rendered_text.clone()))
                    .collect();
                let mut planned = super::redaction_tests::planned(
                    messages,
                    if summarize {
                        StepStrategy::Summarize { chunks: 1 }
                    } else {
                        StepStrategy::Direct
                    },
                );
                planned.task.evidence_sources = sources;
                planned.task.project =
                    retrospective::Project::from_directories(["/private/original/app"]);
                let mut returned = fixture::result();
                returned.sessions[0].project =
                    retrospective::Project::from_directories(["/impostor/app"]);
                let mut responses = Vec::new();
                if summarize {
                    responses.push(Ok("original evidence retained in selected fragment".into()));
                }
                responses.push(Ok(serde_json::to_string(&returned).unwrap()));
                let (provider, recording) = fake_provider(kind.clone(), true, responses);
                let output = run_planned_task(
                    planned,
                    &provider,
                    "synthetic-auth",
                    &super::tests::sample_limits(1000, 100_000),
                    &lang,
                )
                .await
                .unwrap();
                let session = &output.analysis.sessions[0];
                assert_eq!(
                    session.project.as_ref().unwrap().directory,
                    "/private/original/app"
                );
                assert_eq!(
                    session.task_title.as_ref().unwrap().evidence[0]
                        .location
                        .as_ref()
                        .unwrap()
                        .line,
                    1
                );
                assert_eq!(session.questions.len(), 1);
                assert_eq!(session.follow_ups[0].updates.len(), 2);
                assert_eq!(
                    session.follow_ups[0].updates[1].evidence[0].role,
                    "ASSISTANT"
                );
                for payload in recording.payloads() {
                    assert!(!payload.contains("/private/original/app"));
                    assert!(!payload.contains("/impostor/app"));
                }
            }
        }
    }
}
