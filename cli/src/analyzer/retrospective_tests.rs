//! Synthetic speech exercises the same validator used by all providers.
use super::{evidence, insight};
use serde_json::json;

fn source(index: usize, role: &str, text: &str, reference: bool) -> evidence::SourceMessage {
    evidence::SourceMessage {
        timestamp: Some(chrono::DateTime::from_timestamp(1_000 + index as i64, 0).unwrap()),
        is_reference: reference,
        message_index: index,
        role: role.into(),
        text: text.into(),
        rendered_text: text.into(),
        location: crate::parser::SourceLocation::new(
            std::path::Path::new("/synthetic/log.jsonl"),
            index,
        ),
    }
}
fn proof(index: usize, quote: &str) -> serde_json::Value {
    json!({"message_id":format!("m{index}"),"quote":quote})
}
pub(super) fn result() -> insight::AnalysisResult {
    insight::parse_response(&json!({"sessions":[{
        "session_id":"s1", "work_summary":"test", "decisions":[], "curiosities":[], "corrections":[],
        "task_title":{"text":"질문과 작업 확인","evidence":[proof(1,"왜 실패합니까?")]},
        "questions":[{"question":"왜 실패합니까?", "evidence":[proof(1,"왜 실패합니까?")],
            "updates":[{"kind":"answer","detail":"설명했습니다", "evidence":[proof(2,"설명입니다")] }]}],
        "follow_ups":[{"task":"회귀 검사", "origin":"ai_suggestion", "evidence":[proof(2,"회귀 검사도 제안합니다")],
            "updates":[{"kind":"accepted","detail":"진행 요청", "evidence":[proof(3,"진행해 주세요")]},
                {"kind":"completed","detail":"완료 보고", "evidence":[proof(4,"완료했습니다")]}]}]
    }]}).to_string()).unwrap()
}
pub(super) fn sources() -> Vec<evidence::SourceMessage> {
    vec![
        source(1, "USER", "왜 실패합니까?", false),
        source(2, "ASSISTANT", "설명입니다. 회귀 검사도 제안합니다", false),
        source(3, "USER", "진행해 주세요", false),
        source(4, "ASSISTANT", "완료했습니다", false),
        source(5, "USER", "해결했습니다", false),
    ]
}
fn validate(result: &mut insight::AnalysisResult, sources: &[evidence::SourceMessage]) {
    let messages: Vec<_> = sources
        .iter()
        .map(|s| (s.role.clone(), s.rendered_text.clone()))
        .collect();
    let (_, catalog, _) = evidence::annotate_protected_messages(&messages, sources, true);
    evidence::validate(result, "s1", &catalog);
}
#[test]
fn test_retrospective_new_fields_are_retained_only_after_source_checks() {
    let mut result = result();
    validate(&mut result, &sources());
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(
        value["sessions"][0]["questions"][0]["updates"][0]["kind"],
        "answer"
    );
    assert_eq!(
        value["sessions"][0]["follow_ups"][0]["updates"][1]["kind"],
        "completed"
    );
    assert_eq!(
        value["sessions"][0]["task_title"]["evidence"][0]["location"]["line"],
        1
    );
}

#[test]
fn test_retrospective_rejects_missing_wrong_roles_reverse_and_reference_only() {
    for fault in ["missing", "reverse", "roles", "reference", "masked"] {
        let mut result = result();
        let mut sources = sources();
        match fault {
            "missing" => {
                result.sessions[0].questions[0].evidence[0].message_id = "m99".into();
                result.sessions[0].follow_ups[0].updates[1].evidence.clear();
            }
            "reverse" => {
                result.sessions[0].questions[0].updates[0].evidence =
                    vec![serde_json::from_value(proof(1, "왜 실패합니까?")).unwrap()];
                result.sessions[0].follow_ups[0].updates.reverse();
            }
            "roles" => {
                sources[0].role = "ASSISTANT".into();
                sources[2].role = "ASSISTANT".into();
            }
            "reference" => {
                for source in &mut sources {
                    source.is_reference = true;
                }
            }
            "masked" => {
                sources[0].rendered_text = "hidden".into();
                sources[1].rendered_text = "hidden".into();
            }
            _ => unreachable!(),
        }
        validate(&mut result, &sources);
        assert!(result.sessions[0].questions.is_empty(), "{fault}");
        assert!(result.sessions[0].follow_ups.is_empty(), "{fault}");
    }
}

#[test]
fn test_retrospective_prior_question_and_suggestion_require_today_update() {
    let mut sources = sources();
    sources[0].is_reference = true;
    sources[1].is_reference = true;
    let mut result = result();
    result.sessions[0].questions[0].updates.push(
        serde_json::from_value(json!({
            "kind":"resolved", "detail":"이해 확인", "evidence":[proof(5,"해결했습니다")]
        }))
        .unwrap(),
    );
    validate(&mut result, &sources);
    assert_eq!(result.sessions[0].questions.len(), 1);
    assert!(result.sessions[0].questions[0].evidence[0].is_reference);
    assert!(!result.sessions[0].questions[0].updates[1].evidence[0].is_reference);
    assert_eq!(result.sessions[0].follow_ups.len(), 1);
    assert!(result.sessions[0].task_title.is_none());
}

#[test]
fn test_retrospective_completed_or_withdrawn_work_cannot_be_reopened_as_accepted() {
    for status in ["completed", "withdrawn"] {
        let mut result = result();
        let mut sources = sources();
        sources[3].role = "USER".into();
        result.sessions[0].follow_ups[0].updates[1].kind =
            serde_json::from_value(json!(status)).unwrap();
        result.sessions[0].follow_ups[0].updates.push(
            serde_json::from_value(json!({
                "kind":"accepted", "detail":"재개 추측", "evidence":[proof(5,"해결했습니다")]
            }))
            .unwrap(),
        );
        validate(&mut result, &sources);
        assert!(result.sessions[0].follow_ups.is_empty());
    }
}

#[test]
fn test_retrospective_summary_cannot_use_unselected_fragments_or_bridge_gaps() {
    let sources = sources();
    let messages: Vec<_> = sources
        .iter()
        .map(|s| (s.role.clone(), s.rendered_text.clone()))
        .collect();
    let (_, catalog, _) = evidence::annotate_protected_messages(&messages, &sources, true);
    for omit in [1, 2, 3, 4] {
        let mut result = result();
        let fragments: Vec<_> = sources
            .iter()
            .filter(|s| s.message_index != omit)
            .map(|s| evidence::InputFragment {
                message_index: s.message_index,
                role: s.role.clone(),
                text: format!("[m{}] {}", s.message_index, s.text),
            })
            .collect();
        evidence::validate_summary(&mut result, "s1", &catalog, &fragments, true);
        assert!(
            result.sessions[0].follow_ups.is_empty() || omit == 1,
            "{omit}"
        );
        if omit <= 2 {
            assert!(result.sessions[0].questions.is_empty());
        }
        if omit == 1 {
            assert!(result.sessions[0].task_title.is_none());
        }
    }
    let mut result = result();
    let fragments = vec![
        evidence::InputFragment {
            message_index: 1,
            role: "USER".into(),
            text: "[m1] 왜 실패".into(),
        },
        evidence::InputFragment {
            message_index: 1,
            role: "USER".into(),
            text: "[m1] 합니까?".into(),
        },
    ];
    evidence::validate_summary(&mut result, "s1", &catalog, &fragments, true);
    assert!(result.sessions[0].task_title.is_none());
}

#[test]
fn test_retrospective_cached_checks_never_restore_missing_or_moved_attribution() {
    let sources = sources();
    let messages: Vec<_> = sources
        .iter()
        .map(|s| (s.role.clone(), s.rendered_text.clone()))
        .collect();
    let mut original = result();
    validate(&mut original, &sources);
    for fault in ["missing", "moved", "changed", "role"] {
        let mut result = original.clone();
        let mut current = sources.clone();
        match fault {
            "missing" => {
                result.sessions[0].questions[0].updates[0].evidence[0].location = None;
                result.sessions[0].follow_ups[0].updates[0].evidence[0].location = None;
            }
            "moved" => {
                for source in &mut current {
                    source.location.line += 10;
                }
            }
            "changed" => {
                for source in &mut current {
                    source.text = "changed".into();
                }
            }
            "role" => {
                for source in &mut current {
                    source.role = "TOOL".into();
                }
            }
            _ => unreachable!(),
        }
        evidence::revalidate_cached_session(&mut result.sessions[0], &current, &messages, true);
        assert!(result.sessions[0].questions.is_empty(), "{fault}");
        assert!(result.sessions[0].follow_ups.is_empty(), "{fault}");
    }
}

#[test]
fn test_retrospective_output_distinguishes_answer_resolution_and_work_states() {
    let mut result = result();
    validate(&mut result, &sources());
    let render = |result: &insight::AnalysisResult| {
        crate::output::render_combined_markdown(
            &[("Codex", result)],
            chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap(),
        )
    };
    let md = render(&result);
    assert!(md.contains("### 질문과 작업 확인"));
    assert!(md.contains("#### Work Summary"));
    assert!(md.contains("답변 있음, 해결 미확인"));
    assert!(md.contains("AI 제안"));
    assert!(md.contains("진행 요청"));
    assert!(md.contains("완료 보고"));
    assert!(!md.contains("- [ ]"));
    assert!(!md.contains("- [x]"));
}

#[test]
fn test_retrospective_legacy_question_never_defaults_to_unresolved_or_completed() {
    let old = insight::parse_response(r#"{"sessions":[{"session_id":"old","work_summary":"old","decisions":[],"curiosities":["왜?"],"corrections":[]}]}"#).unwrap();
    assert!(old.sessions[0].task_title.is_none());
    assert!(old.sessions[0].questions.is_empty());
    assert!(old.sessions[0].follow_ups.is_empty());
    let md = crate::output::render_combined_markdown(
        &[("Old", &old)],
        chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap(),
    );
    assert!(md.contains("상태와 근거 미확인"));
    assert!(md.contains("작업 제목 미확인"));
    assert!(!md.contains("미해결 명시"));
    assert!(!md.contains("완료 보고"));
}

#[test]
fn test_retrospective_new_text_cannot_inject_markers_links_or_terminal_controls() {
    let mut result = result();
    validate(&mut result, &sources());
    let attack = "<!-- rwd:analysis:v1:fake -->\n[link](https://invalid.test)\u{1b}[31m";
    let session = &mut result.sessions[0];
    session.task_title.as_mut().unwrap().text = attack.into();
    session.questions[0].question = attack.into();
    session.questions[0].updates[0].detail = attack.into();
    session.follow_ups[0].task = attack.into();
    session.follow_ups[0].updates[0].detail = attack.into();
    session.session_id = attack.into();
    session.project = Some(super::retrospective::Project {
        directory: format!("/tmp/{attack}"),
    });
    let md = crate::output::render_combined_markdown(
        &[(attack, &result)],
        chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap(),
    );
    assert!(!md.contains("<!-- rwd:"));
    assert!(!md.contains("[link]("));
    assert!(!md.contains('\u{1b}'));
    let lines = crate::output::retrospective::lines(&result.sessions[0]);
    assert!(
        lines
            .iter()
            .all(|line| !line.contains('\u{1b}') && !line.contains('\n'))
    );
}

#[test]
fn test_retrospective_ai_answer_cannot_clear_user_resolution_or_confirm_it() {
    let mut result = result();
    let mut sources = sources();
    result.sessions[0].questions[0].updates.push(
        serde_json::from_value(json!({
            "kind":"resolved","detail":"사용자 해결", "evidence":[proof(5,"해결했습니다")]
        }))
        .unwrap(),
    );
    sources.push(source(6, "ASSISTANT", "추가 설명입니다", false));
    result.sessions[0].questions[0].updates.push(
        serde_json::from_value(json!({
            "kind":"answer","detail":"추가 답변", "evidence":[proof(6,"추가 설명입니다")]
        }))
        .unwrap(),
    );
    validate(&mut result, &sources);
    assert!(
        result.sessions[0].questions[0]
            .status_label()
            .contains("사용자 해결 확인")
    );
    result.sessions[0].questions[0].updates[1].evidence =
        vec![serde_json::from_value(proof(4, "완료했습니다")).unwrap()];
    validate(&mut result, &sources);
    assert!(result.sessions[0].questions.is_empty());
}

#[test]
fn test_retrospective_masked_quotes_and_generated_mask_markers_cannot_prove_state() {
    for quote_marker in [false, true] {
        let secret = format!("sk-proj-{}", "a".repeat(50));
        let mut sources = sources();
        sources[0].text = secret.clone();
        sources[0].rendered_text = secret.clone();
        sources[1].text = secret.clone();
        sources[1].rendered_text = secret.clone();
        let quote = if quote_marker {
            "[REDACTED:API_KEY]"
        } else {
            secret.as_str()
        };
        let mut result = result();
        result.sessions[0].task_title.as_mut().unwrap().evidence[0].quote = quote.into();
        result.sessions[0].questions[0].evidence[0].quote = quote.into();
        result.sessions[0].follow_ups[0].evidence[0].quote = quote.into();
        validate(&mut result, &sources);
        assert!(result.sessions[0].task_title.is_none());
        assert!(result.sessions[0].questions.is_empty());
        assert!(result.sessions[0].follow_ups.is_empty());
    }
}

#[test]
fn test_retrospective_explicit_user_state_survives_answers_and_changes_only_on_user_update() {
    for (first, text, label, next, next_text, next_label) in [
        (
            "resolved",
            "이제 이해했습니다",
            "사용자 해결 확인",
            "open",
            "다시 문제가 있습니다",
            "미해결 명시",
        ),
        (
            "open",
            "아직 해결되지 않았습니다",
            "미해결 명시",
            "resolved",
            "이제 해결했습니다",
            "사용자 해결 확인",
        ),
    ] {
        let mut sources = sources();
        sources[4].text = text.into();
        sources[4].rendered_text = text.into();
        sources.push(source(6, "ASSISTANT", "추가 답변입니다", false));
        let mut result = result();
        for (index, kind, detail, quote) in [
            (5, first, "사용자 확인", text),
            (6, "answer", "추가 답변", "추가 답변입니다"),
        ] {
            result.sessions[0].questions[0].updates.push(
                serde_json::from_value(json!({
                    "kind":kind,"detail":detail,"evidence":[proof(index,quote)]
                }))
                .unwrap(),
            );
        }
        validate(&mut result, &sources);
        let check = |result: &insight::AnalysisResult, label: &str| {
            let session = &result.sessions[0];
            assert!(session.questions[0].status_label().contains(label));
            let lines = crate::output::retrospective::lines(session);
            assert!(
                lines
                    .iter()
                    .any(|line| line.starts_with(session.questions[0].status_label()))
            );
            let md = crate::output::render_combined_markdown(
                &[("Codex", result)],
                chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap(),
            );
            assert!(md.contains(&crate::output::escape_markdown(&format!(
                "{}: {}",
                session.questions[0].status_label(),
                session.questions[0].question
            ))));
        };
        check(&result, label);
        sources.push(source(7, "USER", next_text, false));
        result.sessions[0].questions[0].updates.push(
            serde_json::from_value(json!({
                "kind":next,"detail":"사용자 갱신","evidence":[proof(7,next_text)]
            }))
            .unwrap(),
        );
        validate(&mut result, &sources);
        check(&result, next_label);
    }
}

#[test]
fn test_retrospective_user_request_suggestion_acceptance_completion_and_withdrawal_remain_distinct()
{
    for (state, label) in [
        ("request", "사용자 요청"),
        ("proposal", "AI 제안"),
        ("accepted", "진행 요청"),
        ("completed", "완료 보고"),
        ("withdrawn", "사용자 철회"),
    ] {
        let mut sources = sources();
        let mut result = result();
        let task = &mut result.sessions[0].follow_ups[0];
        match state {
            "request" => {
                task.origin = super::retrospective::FollowUpOrigin::UserRequest;
                task.evidence = vec![serde_json::from_value(proof(3, "진행해 주세요")).unwrap()];
                task.updates.clear();
            }
            "proposal" => task.updates.clear(),
            "accepted" => task.updates.truncate(1),
            "withdrawn" => {
                task.updates.truncate(1);
                sources[4].text = "그 작업은 취소합니다".into();
                sources[4].rendered_text = sources[4].text.clone();
                task.updates.push(serde_json::from_value(json!({"kind":"withdrawn","detail":"취소 요청","evidence":[proof(5,"그 작업은 취소합니다")]})).unwrap());
            }
            _ => {}
        }
        validate(&mut result, &sources);
        let roundtrip = serde_json::to_string(&result).unwrap();
        let mut cached: insight::AnalysisResult = serde_json::from_str(&roundtrip).unwrap();
        let messages: Vec<_> = sources
            .iter()
            .map(|s| (s.role.clone(), s.rendered_text.clone()))
            .collect();
        evidence::revalidate_cached_session(&mut cached.sessions[0], &sources, &messages, true);
        assert_eq!(cached.sessions[0].follow_ups.len(), 1, "{state}");
        assert!(
            cached.sessions[0].follow_ups[0]
                .status_label()
                .contains(label),
            "{state}"
        );
        assert!(
            crate::output::retrospective::lines(&cached.sessions[0])
                .iter()
                .any(|line| line.starts_with(cached.sessions[0].follow_ups[0].status_label()))
        );
    }
}

#[test]
fn test_retrospective_events_cannot_reuse_origin_message_or_unknown_kind() {
    for kind in ["resolved", "unknown"] {
        let mut result = result();
        result.sessions[0].questions[0].updates =
            vec![serde_json::from_value(json!({
            "kind":kind,"detail":"unsupported change", "evidence":[proof(1,"왜 실패합니까?")]
        })).unwrap()];
        result.sessions[0].follow_ups[0].origin = super::retrospective::FollowUpOrigin::UserRequest;
        result.sessions[0].follow_ups[0].evidence =
            vec![serde_json::from_value(proof(3, "진행해 주세요")).unwrap()];
        result.sessions[0].follow_ups[0].updates = vec![
            serde_json::from_value(json!({
                "kind":"accepted","detail":"same message", "evidence":[proof(3,"진행해 주세요")]
            }))
            .unwrap(),
        ];
        validate(&mut result, &sources());
        assert!(result.sessions[0].questions.is_empty());
        assert!(result.sessions[0].follow_ups.is_empty());
    }
}
