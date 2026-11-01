//! Local attribution checks. These verify quotations, not the meaning of an insight.

use super::insight::{AnalysisResult, CorrectionKind, Evidence, SessionInsight};
use super::prompt::{TimedLine, codex_role_text, extract_entry_text, splice_timed_items};
use crate::parser::SourceLocation;
use crate::parser::claude::{ContentBlock, LogEntry};
use crate::parser::codex::CodexEntry;
use std::collections::HashMap;

/// Only visible user/assistant text, before log compaction. Never tool output.
#[derive(Debug, Clone)]
pub struct SourceMessage {
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub is_reference: bool,
    /// One-based position in SessionTask::messages, including non-speech entries.
    pub message_index: usize,
    pub role: String,
    pub text: String,
    pub rendered_text: String,
    pub location: SourceLocation,
}

/// Local provenance survives splitting even when unlocated text contains fake IDs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputFragment {
    pub message_index: usize,
    pub role: String,
    pub text: String,
}

/// Only original visible speech can support quotations, not thinking, metadata,
/// tool markers, or injected compact summaries. Keep the existing prompt intact.
pub fn claude_source_messages(entries: &[LogEntry], lines: &[TimedLine]) -> Vec<SourceMessage> {
    let items = entries.iter().filter_map(|entry| {
        extract_entry_text(entry)?;
        Some((
            crate::parser::claude::entry_timestamp(entry),
            claude_source_message(entry),
        ))
    });
    indexed_sources(splice_timed_items(items, lines, |_| None))
}

fn claude_source_message(entry: &LogEntry) -> Option<SourceMessage> {
    let (location, text) = match entry {
        LogEntry::User(e) if !e.is_meta && !e.is_compact_summary => {
            let content = e.message.as_ref()?.get("content")?;
            let text = if let Some(text) = content.as_str() {
                text.to_string()
            } else {
                content
                    .as_array()?
                    .iter()
                    .filter(|b| b["type"].as_str() == Some("text"))
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            (e.source_location.as_ref()?, text)
        }
        LogEntry::Assistant(e) => {
            let text = e
                .message
                .as_ref()?
                .content
                .iter()
                .filter_map(|block| {
                    if let ContentBlock::Text { text } = block {
                        text.as_deref()
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            (e.source_location.as_ref()?, text)
        }
        _ => return None,
    };
    if text.trim().is_empty() {
        return None;
    }
    let (role, rendered_text) = extract_entry_text(entry)?;
    Some(SourceMessage {
        timestamp: crate::parser::claude::entry_timestamp(entry),
        is_reference: false,
        message_index: 0,
        role: role.into(),
        text,
        rendered_text,
        location: location.clone(),
    })
}

pub fn codex_source_messages(entries: &[CodexEntry], lines: &[TimedLine]) -> Vec<SourceMessage> {
    let items = entries.iter().filter_map(|entry| {
        codex_role_text(entry)?;
        Some((
            crate::parser::codex::entry_timestamp(entry),
            codex_source_message(entry),
        ))
    });
    indexed_sources(splice_timed_items(items, lines, |_| None))
}

fn codex_source_message(entry: &CodexEntry) -> Option<SourceMessage> {
    let (text, location) = match entry {
        CodexEntry::UserMessage {
            text,
            source_location,
            ..
        }
        | CodexEntry::AssistantMessage {
            text,
            source_location,
            ..
        } => (text, source_location.as_ref()?),
        _ => return None,
    };
    if text.trim().is_empty() {
        return None;
    }
    let (role, rendered_text) = codex_role_text(entry)?;
    Some(SourceMessage {
        timestamp: crate::parser::codex::entry_timestamp(entry),
        is_reference: false,
        message_index: 0,
        role,
        text: text.clone(),
        rendered_text,
        location: location.clone(),
    })
}

fn indexed_sources(items: Vec<Option<SourceMessage>>) -> Vec<SourceMessage> {
    items
        .into_iter()
        .enumerate()
        .filter_map(|(index, source)| {
            source.map(|mut source| {
                source.message_index = index + 1;
                source
            })
        })
        .collect()
}

/// IDs refer only to file-backed messages; metadata and subagent summaries get no IDs.
pub fn annotate_messages(
    messages: &[(String, String)],
    sources: &[SourceMessage],
) -> (Vec<(String, String)>, HashMap<String, SourceMessage>) {
    let mut catalog = HashMap::new();
    let by_index: HashMap<_, _> = sources.iter().map(|s| (s.message_index, s)).collect();
    let annotated = messages
        .iter()
        .enumerate()
        .map(|(index, (role, text))| {
            let Some(&source) = by_index.get(&(index + 1)) else {
                return (role.clone(), text.clone());
            };
            if source.role != *role || source.rendered_text != *text {
                return (role.clone(), text.clone());
            }
            let id = format!("m{}", index + 1);
            catalog.insert(id.clone(), source.clone());
            (role.clone(), format!("[{id}] {text}"))
        })
        .collect();
    (annotated, catalog)
}

/// Fail closed on missing IDs, empty quotes, and quotations absent from either
/// the original visible text or the compacted/masked analysis input.
pub fn validate(
    result: &mut AnalysisResult,
    session_id: &str,
    catalog: &HashMap<String, SourceMessage>,
) {
    result
        .sessions
        .retain(|session| session.session_id == session_id);
    for session in &mut result.sessions {
        session.decisions.retain_mut(|item| {
            resolve(&mut item.evidence, catalog) && has_target_user(&item.evidence)
        });
        session.til.retain_mut(|item| {
            resolve(&mut item.evidence, catalog) && has_target_user(&item.evidence)
        });
        session.corrections.retain_mut(|item| {
            resolve(&mut item.evidence, catalog)
                && has_target_user(&item.evidence)
                && correction_order_matches(&item.evidence, catalog)
        });
    }
}

/// In addition to original-source checks, require every quote to occur inside
/// one actually selected fragment of that same message. Never bridge a gap.
pub fn validate_summary(
    result: &mut AnalysisResult,
    session_id: &str,
    catalog: &HashMap<String, SourceMessage>,
    fragments: &[InputFragment],
    redactor_enabled: bool,
) {
    let mut allowed: HashMap<String, Vec<String>> = HashMap::new();
    for fragment in fragments {
        let id = format!("m{}", fragment.message_index);
        let Some(source) = catalog.get(&id) else {
            continue;
        };
        if fragment.role != source.role {
            continue;
        }
        let Some(payload) = fragment.text.strip_prefix(&format!("[{id}] ")) else {
            continue;
        };
        let payload = if redactor_enabled {
            crate::redactor::redact_text(payload).0
        } else {
            payload.to_string()
        };
        allowed.entry(id).or_default().push(payload);
    }
    let supported = |proofs: &[Evidence]| {
        proofs.iter().all(|proof| {
            allowed
                .get(&proof.message_id)
                .is_some_and(|parts| parts.iter().any(|part| part.contains(&proof.quote)))
        })
    };
    for session in &mut result.sessions {
        session.decisions.retain(|item| supported(&item.evidence));
        session.til.retain(|item| supported(&item.evidence));
        session.corrections.retain(|item| supported(&item.evidence));
    }
    validate(result, session_id, catalog);
}

fn has_target_user(evidence: &[Evidence]) -> bool {
    evidence.iter().any(|e| e.role == "USER" && !e.is_reference)
}

fn correction_order_matches(
    evidence: &[Evidence],
    catalog: &HashMap<String, SourceMessage>,
) -> bool {
    let latest_model = evidence
        .iter()
        .filter(|proof| proof.role == "ASSISTANT")
        .filter_map(|proof| catalog.get(&proof.message_id))
        .map(|source| source.message_index)
        .max();
    let first_user = evidence
        .iter()
        .filter(|proof| proof.role == "USER")
        .filter_map(|proof| catalog.get(&proof.message_id))
        .map(|source| source.message_index)
        .min();
    // Every cited model statement must precede the correcting user speech.
    matches!((latest_model, first_user), (Some(model), Some(user)) if model < user)
}

fn resolve(evidence: &mut [Evidence], catalog: &HashMap<String, SourceMessage>) -> bool {
    if evidence.is_empty() {
        return false;
    }
    for proof in evidence {
        // Discard model-provided attribution even when a later check fails.
        proof.location = None;
        proof.role.clear();
        proof.timestamp = None;
        proof.is_reference = false;
        let Some(source) = catalog.get(&proof.message_id) else {
            return false;
        };
        if !quote_matches(proof, source) {
            return false;
        }
        proof.location = Some(source.location.clone());
        proof.role.clone_from(&source.role);
        proof.timestamp = source.timestamp;
        proof.is_reference = source.is_reference;
    }
    true
}

fn quote_matches(proof: &Evidence, source: &SourceMessage) -> bool {
    !proof.quote.trim().is_empty()
        && source.text.contains(&proof.quote)
        && source.rendered_text.contains(&proof.quote)
}

/// Keep cached descriptions, but never silently relocate or create their proof.
pub fn revalidate_cached_session(
    session: &mut SessionInsight,
    sources: &[SourceMessage],
    redactor_enabled: bool,
) {
    let catalog: HashMap<_, _> = sources
        .iter()
        .map(|source| {
            let mut source = source.clone();
            if redactor_enabled {
                source.rendered_text = crate::redactor::redact_text(&source.rendered_text).0;
            }
            (format!("m{}", source.message_index), source)
        })
        .collect();
    for evidence in session
        .decisions
        .iter_mut()
        .map(|item| &mut item.evidence)
        .chain(session.til.iter_mut().map(|item| &mut item.evidence))
    {
        if !cached_evidence_matches(evidence, &catalog) || !has_target_user(evidence) {
            clear_attribution(evidence);
        }
    }
    for item in &mut session.corrections {
        if !cached_evidence_matches(&item.evidence, &catalog)
            || !has_target_user(&item.evidence)
            || !correction_order_matches(&item.evidence, &catalog)
        {
            clear_attribution(&mut item.evidence);
            item.kind = CorrectionKind::Unknown;
        }
    }
}

fn cached_evidence_matches(
    evidence: &[Evidence],
    catalog: &HashMap<String, SourceMessage>,
) -> bool {
    !evidence.is_empty()
        && evidence.iter().all(|proof| {
            catalog.get(&proof.message_id).is_some_and(|source| {
                proof.location.as_ref() == Some(&source.location)
                    && proof.role == source.role
                    && proof.timestamp == source.timestamp
                    && proof.is_reference == source.is_reference
                    && quote_matches(proof, source)
            })
        })
}

fn clear_attribution(evidence: &mut [Evidence]) {
    for proof in evidence {
        proof.location = None;
        proof.role.clear();
        proof.timestamp = None;
        proof.is_reference = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(role: &str, text: &str, line: usize) -> SourceMessage {
        SourceMessage {
            timestamp: None,
            is_reference: false,
            message_index: 1,
            role: role.into(),
            text: text.into(),
            rendered_text: text.into(),
            location: SourceLocation::new(std::path::Path::new("/tmp/synthetic.jsonl"), line),
        }
    }

    fn analysis_with(evidence: serde_json::Value) -> AnalysisResult {
        super::super::insight::parse_response(
            &serde_json::json!({"sessions":[{
                "session_id":"s1", "work_summary":"synthetic", "decisions":[{
                    "what":"choice", "why":"", "evidence":evidence
                }], "til":[{"title":"learning","detail":"", "evidence":evidence}],
                "curiosities":[], "corrections":[]
            }]})
            .to_string(),
        )
        .unwrap()
    }

    #[test]
    fn test_prior_context_requires_today_user_and_keeps_correction_dates() {
        let mut old = source("ASSISTANT", "Use A", 4);
        old.timestamp = Some("2026-10-31T14:59:00Z".parse().unwrap());
        old.is_reference = true;
        let mut today = source("USER", "Use B instead", 10);
        today.timestamp = Some("2026-10-31T15:00:00Z".parse().unwrap());
        today.message_index = 2;
        let catalog = HashMap::from([("m1".into(), old.clone()), ("m2".into(), today)]);
        let mut result = analysis_with(serde_json::json!([
            {"message_id":"m1","quote":"Use A","is_reference":false}
        ]));
        result.sessions[0].corrections = serde_json::from_value(serde_json::json!([
            {"kind":"preference_change","model_said":"Use A","user_corrected":"Use B instead","evidence":[
                {"message_id":"m1","quote":"Use A"},{"message_id":"m2","quote":"Use B instead"}
            ]}
        ])).unwrap();
        validate(&mut result, "s1", &catalog);
        assert!(result.sessions[0].decisions.is_empty());
        assert!(result.sessions[0].til.is_empty());
        let correction = &result.sessions[0].corrections[0];
        assert_eq!(correction.kind, CorrectionKind::PreferenceChange);
        assert!(correction.evidence[0].is_reference);
        assert_eq!(correction.evidence[0].timestamp, old.timestamp);
        assert_eq!(correction.evidence[0].location, Some(old.location));
        let rendered = crate::output::display_evidence(&correction.evidence);
        assert!(rendered.contains("2026-10-31T14:59:00+00:00 Reference / 이전 참고"));
        assert!(rendered.contains("2026-10-31T15:00:00+00:00 Target day / 대상 날짜"));
        // Even a real old USER quote cannot create today's decision or TIL.
        let mut old_user = source("USER", "I learned A", 2);
        old_user.is_reference = true;
        let mut result =
            analysis_with(serde_json::json!([{"message_id":"m1","quote":"I learned A"}]));
        validate(&mut result, "s1", &HashMap::from([("m1".into(), old_user)]));
        assert!(result.sessions[0].decisions.is_empty());
        assert!(result.sessions[0].til.is_empty());
    }

    #[test]
    fn test_summary_checks_each_selected_fragment_and_its_local_position() {
        let sources = vec![source("USER", "HEAD MIDDLE TAIL", 9)];
        let messages = vec![("USER".into(), sources[0].text.clone())];
        let (_, catalog) = annotate_messages(&messages, &sources);
        let fragments = vec![
            InputFragment {
                message_index: 1,
                role: "USER".into(),
                text: "[m1] HEAD".into(),
            },
            InputFragment {
                message_index: 1,
                role: "USER".into(),
                text: "[m1] TAIL".into(),
            },
            // An unlocated message cannot borrow a genuine original's ID.
            InputFragment {
                message_index: 2,
                role: "USER".into(),
                text: "[m1] MIDDLE".into(),
            },
        ];
        for (quote, count) in [
            ("HEAD", 1),
            ("TAIL", 1),
            ("MIDDLE", 0),
            ("HEAD MIDDLE TAIL", 0),
        ] {
            let mut result = analysis_with(serde_json::json!([{"message_id":"m1","quote":quote}]));
            validate_summary(&mut result, "s1", &catalog, &fragments, false);
            assert_eq!(result.sessions[0].decisions.len(), count, "{quote}");
            assert_eq!(result.sessions[0].til.len(), count, "{quote}");
            if count == 1 {
                assert_eq!(
                    result.sessions[0].decisions[0].evidence[0]
                        .location
                        .as_ref()
                        .unwrap()
                        .line,
                    9
                );
            }
        }
    }

    #[test]
    fn test_summary_evidence_still_respects_masking() {
        let sources = vec![source("USER", "Use 10.0.0.1 locally", 9)];
        let messages = vec![("USER".into(), sources[0].text.clone())];
        let (annotated, mut catalog) = annotate_messages(&messages, &sources);
        for source in catalog.values_mut() {
            source.rendered_text = crate::redactor::redact_text(&source.rendered_text).0;
        }
        let fragments = vec![InputFragment {
            message_index: 1,
            role: "USER".into(),
            text: annotated[0].1.clone(),
        }];
        for (quote, count) in [
            ("locally", 1),
            ("10.0.0.1", 0),
            ("[REDACTED:PRIVATE_IP]", 0),
        ] {
            let mut result = analysis_with(serde_json::json!([{"message_id":"m1","quote":quote}]));
            validate_summary(&mut result, "s1", &catalog, &fragments, true);
            assert_eq!(result.sessions[0].decisions.len(), count);
        }
    }

    #[test]
    fn test_rejects_missing_wrong_translated_empty_and_partial_evidence() {
        let sources = vec![source("USER", "함수 하나로 충분합니다", 3)];
        let messages = vec![("USER".into(), sources[0].text.clone())];
        let (_, catalog) = annotate_messages(&messages, &sources);
        for evidence in [
            serde_json::json!([]),
            serde_json::json!([{"message_id":"m99","quote":"함수 하나로 충분합니다"}]),
            serde_json::json!([{"message_id":"m1","quote":"One function is enough"}]),
            serde_json::json!([{"message_id":"m1","quote":" "}]),
            serde_json::json!([{"message_id":"m1","quote":"함수 하나로 충분합니다"},
                {"message_id":"m2","quote":"unsupported reason"}]),
        ] {
            let mut result = analysis_with(evidence);
            validate(&mut result, "s1", &catalog);
            assert!(result.sessions[0].decisions.is_empty());
            assert!(result.sessions[0].til.is_empty());
        }
    }

    #[test]
    fn test_assistant_alone_and_wrong_session_cannot_establish_decision_or_learning() {
        let sources = vec![source("ASSISTANT", "Use a function", 4)];
        let (_, catalog) =
            annotate_messages(&[("ASSISTANT".into(), "Use a function".into())], &sources);
        let mut result =
            analysis_with(serde_json::json!([{"message_id":"m1","quote":"Use a function"}]));
        validate(&mut result, "s1", &catalog);
        assert!(result.sessions[0].decisions.is_empty());
        assert!(result.sessions[0].til.is_empty());
        validate(&mut result, "other_session", &catalog);
        assert!(result.sessions.is_empty());
    }

    #[test]
    fn test_correction_kinds_preserved_and_both_speakers_required() {
        use super::super::insight::CorrectionKind;
        // Canned responses test the contract, not an AI's classification accuracy.
        for (kind, expected, model, user) in [
            (
                "factual_error",
                CorrectionKind::FactualError,
                "2 + 2 = 5",
                "2 + 2 = 4",
            ),
            (
                "preference_change",
                CorrectionKind::PreferenceChange,
                "Add a framework",
                "함수 하나로 충분합니다",
            ),
            (
                "unknown",
                CorrectionKind::Unknown,
                "Use X",
                "틀렸습니다. 다시 봐주세요",
            ),
            (
                "new_kind",
                CorrectionKind::Unknown,
                "Use X",
                "다시 봐주세요",
            ),
        ] {
            let mut sources = vec![source("ASSISTANT", model, 1), source("USER", user, 2)];
            sources[1].message_index = 2;
            let messages = sources
                .iter()
                .map(|s| (s.role.clone(), s.text.clone()))
                .collect::<Vec<_>>();
            let (_, catalog) = annotate_messages(&messages, &sources);
            let mut result = super::super::insight::parse_response(&serde_json::json!({"sessions":[{
                "session_id":"s1", "work_summary":"synthetic", "decisions":[], "til":[],
                "curiosities":[], "corrections":[{
                    "model_said":model,"user_corrected":user,"kind":kind,
                    "evidence":[{"message_id":"m1","quote":model}, {"message_id":"m2","quote":user}]
                }]
            }]}).to_string()).unwrap();
            validate(&mut result, "s1", &catalog);
            assert_eq!(result.sessions[0].corrections[0].kind, expected);
            result.sessions[0].corrections[0].evidence.remove(0);
            validate(&mut result, "s1", &catalog);
            assert!(result.sessions[0].corrections.is_empty());
        }
    }

    #[test]
    fn test_correction_requires_model_speech_before_user_response() {
        for (model_index, user_index, accepted) in [(2, 10, true), (10, 2, false)] {
            let mut model = source("ASSISTANT", "This feature is supported.", 90);
            model.message_index = model_index;
            let mut user = source("USER", "This feature is not supported.", 3);
            user.message_index = user_index;
            let model_id = format!("m{model_index}");
            let user_id = format!("m{user_index}");
            let mut catalog = HashMap::from([(model_id.clone(), model), (user_id.clone(), user)]);
            // Deliberately list the user quote first. Conversation order, not
            // array order, lexical IDs, or physical line numbers, determines it.
            let mut result = super::super::insight::parse_response(
                &serde_json::json!({"sessions":[{
                    "session_id":"s1", "work_summary":"synthetic", "decisions":[], "til":[],
                    "curiosities":[], "corrections":[{
                        "model_said":"This feature is supported.",
                        "user_corrected":"This feature is not supported.","kind":"factual_error",
                        "evidence":[{"message_id":user_id,"quote":"This feature is not supported."},
                            {"message_id":model_id,"quote":"This feature is supported."}]
                    }]
                }]})
                .to_string(),
            )
            .unwrap();
            validate(&mut result, "s1", &catalog);
            assert_eq!(result.sessions[0].corrections.len(), usize::from(accepted));
            if accepted {
                for (role, index) in [("ASSISTANT", 11), ("USER", 1)] {
                    let mut extra = source(role, "Extra context", 4);
                    extra.message_index = index;
                    let id = format!("m{index}");
                    catalog.insert(id.clone(), extra);
                    let mut mixed = result.clone();
                    mixed.sessions[0].corrections[0].evidence.push(
                        serde_json::from_value(serde_json::json!({
                            "message_id": id, "quote": "Extra context"
                        }))
                        .unwrap(),
                    );
                    validate(&mut mixed, "s1", &catalog);
                    assert!(mixed.sessions[0].corrections.is_empty());
                }
            }
        }
    }

    #[test]
    fn test_cached_evidence_requires_same_id_role_quote_and_physical_location() {
        let original = source("USER", "A function is enough.", 7);
        let catalog = HashMap::from([("m1".into(), original.clone())]);
        let mut result = analysis_with(serde_json::json!([
            {"message_id":"m1", "quote":"A function is enough."}
        ]));
        validate(&mut result, "s1", &catalog);
        let before = result.sessions[0].clone();
        let mut shifted = original.clone();
        shifted.location.line += 1;
        let mut moved = original.clone();
        moved.location.path = "/tmp/moved.jsonl".into();
        let mut edited = original.clone();
        edited.text = "Use a framework instead.".into();
        let mut changed_role = original.clone();
        changed_role.role = "ASSISTANT".into();
        let mut reordered = original.clone();
        reordered.message_index = 2;
        for (name, current, matches) in [
            ("same", vec![original.clone()], true),
            ("shifted", vec![shifted], false),
            ("moved", vec![moved], false),
            ("edited", vec![edited], false),
            ("role changed", vec![changed_role], false),
            ("reordered", vec![reordered], false),
            ("missing", vec![], false),
        ] {
            let mut session = before.clone();
            revalidate_cached_session(&mut session, &current, true);
            assert_eq!(session.decisions[0].what, before.decisions[0].what);
            assert_eq!(session.til[0].title, before.til[0].title);
            for proof in [
                &session.decisions[0].evidence[0],
                &session.til[0].evidence[0],
            ] {
                assert_eq!(proof.location.is_some(), matches, "{name}");
                assert_eq!(proof.quote, "A function is enough.");
                assert_eq!(proof.role.is_empty(), !matches, "{name}");
            }
        }
        let mut legacy = before;
        clear_attribution(&mut legacy.decisions[0].evidence);
        clear_attribution(&mut legacy.til[0].evidence);
        revalidate_cached_session(&mut legacy, &[original], false);
        assert!(legacy.decisions[0].evidence[0].location.is_none());
        assert!(legacy.til[0].evidence[0].location.is_none());
    }

    #[test]
    fn test_cached_item_with_one_invalid_quote_loses_all_attribution() {
        let original = source("USER", "A function is enough. token=synthetic-secret", 7);
        let catalog = HashMap::from([("m1".into(), original.clone())]);
        let mut result = analysis_with(serde_json::json!([
            {"message_id":"m1", "quote":"A function is enough."},
            {"message_id":"m1", "quote":"synthetic-secret"}
        ]));
        validate(&mut result, "s1", &catalog);
        let mut current = original;
        current.rendered_text = "A function is enough. token=[REDACTED]".into();
        revalidate_cached_session(&mut result.sessions[0], &[current], false);
        for evidence in [
            &result.sessions[0].decisions[0].evidence,
            &result.sessions[0].til[0].evidence,
        ] {
            assert_eq!(evidence.len(), 2);
            assert!(
                evidence
                    .iter()
                    .all(|proof| proof.location.is_none() && proof.role.is_empty())
            );
            let displayed = crate::output::display_evidence(evidence);
            assert!(displayed.contains("근거 미확인"));
            assert!(!displayed.contains("synthetic-secret"));
        }
    }

    #[test]
    fn test_cached_attribution_honors_current_masking_setting() {
        let original = source("USER", "password = \"synthetic_value\"", 1);
        let mut result = analysis_with(serde_json::json!([
            {"message_id":"m1", "quote":"synthetic_value"}
        ]));
        validate(
            &mut result,
            "s1",
            &HashMap::from([("m1".into(), original.clone())]),
        );
        for masking_enabled in [false, true] {
            let mut session = result.sessions[0].clone();
            revalidate_cached_session(
                &mut session,
                std::slice::from_ref(&original),
                masking_enabled,
            );
            assert_eq!(
                session.decisions[0].evidence[0].location.is_none(),
                masking_enabled
            );
            assert_eq!(
                session.til[0].evidence[0].location.is_none(),
                masking_enabled
            );
        }
    }

    #[test]
    fn test_cached_correction_downgrades_reversed_speech_and_missing_evidence() {
        let mut model = source("ASSISTANT", "Use X", 1);
        let mut user = source("USER", "Use Y", 2);
        user.message_index = 2;
        let (_, catalog) = annotate_messages(
            &[
                ("ASSISTANT".into(), "Use X".into()),
                ("USER".into(), "Use Y".into()),
            ],
            &[model.clone(), user.clone()],
        );
        let mut result = super::super::insight::parse_response(r#"{"sessions":[{
            "session_id":"s1","work_summary":"synthetic","decisions":[],"curiosities":[],"til":[],
            "corrections":[{"kind":"preference_change","model_said":"Use X","user_corrected":"Use Y",
                "evidence":[{"message_id":"m1","quote":"Use X"},{"message_id":"m2","quote":"Use Y"}]}]
        }]}"#).unwrap();
        validate(&mut result, "s1", &catalog);
        let before = result.sessions[0].clone();
        let mut valid = before.clone();
        revalidate_cached_session(&mut valid, &[model.clone(), user.clone()], true);
        assert_eq!(valid.corrections[0].kind, CorrectionKind::PreferenceChange);
        assert!(
            valid.corrections[0]
                .evidence
                .iter()
                .all(|proof| proof.location.is_some())
        );
        // Mimic a cache produced before chronological validation existed.
        model.message_index = 2;
        user.message_index = 1;
        let mut reversed = before.clone();
        reversed.corrections[0].evidence[0].message_id = "m2".into();
        reversed.corrections[0].evidence[1].message_id = "m1".into();
        for mut session in [before, reversed] {
            revalidate_cached_session(&mut session, &[model.clone(), user.clone()], false);
            assert_eq!(session.corrections.len(), 1);
            assert_eq!(session.corrections[0].kind, CorrectionKind::Unknown);
            assert!(
                session.corrections[0]
                    .evidence
                    .iter()
                    .all(|proof| proof.location.is_none())
            );
        }
    }

    #[test]
    fn test_quotes_must_survive_compaction_and_masking_without_using_synthetic_text() {
        for (original, rendered, quote) in [
            (
                "line A\nline B\nline C",
                "line A\n[omitted]\nline C",
                "line B",
            ),
            (
                "line A\nline B\nline C",
                "line A\n[omitted]\nline C",
                "[omitted]",
            ),
            (
                "token=synthetic-secret",
                "token=[REDACTED]",
                "synthetic-secret",
            ),
            ("token=synthetic-secret", "token=[REDACTED]", "[REDACTED]"),
        ] {
            let mut candidate = source("USER", original, 1);
            candidate.rendered_text = rendered.into();
            let catalog = HashMap::from([("m1".into(), candidate)]);
            let mut result = analysis_with(serde_json::json!([{"message_id":"m1","quote":quote}]));
            validate(&mut result, "s1", &catalog);
            assert!(result.sessions[0].decisions.is_empty());
            assert!(result.sessions[0].til.is_empty());
        }
    }

    #[test]
    fn test_injected_lines_do_not_shift_original_locations_or_get_evidence_ids() {
        let mut sources = vec![source("USER", "choice", 9), source("ASSISTANT", "done", 11)];
        sources[1].message_index = 3;
        let messages = vec![
            ("USER".into(), "choice".into()),
            ("SUBAGENT_RESULT".into(), "report".into()),
            ("ASSISTANT".into(), "done".into()),
        ];
        let (annotated, catalog) = annotate_messages(&messages, &sources);
        assert_eq!(annotated[1].1, "report");
        assert!(!catalog.contains_key("m2"));
        assert_eq!(catalog["m3"].location.line, 11);
    }

    #[test]
    fn test_valid_learning_survives_cache_and_renders_quote_and_original_position() {
        let sources = vec![source(
            "USER",
            "이제 이해했습니다. 시간대가 달라 키도 달라졌습니다.",
            27,
        )];
        let (_, catalog) = annotate_messages(&[("USER".into(), sources[0].text.clone())], &sources);
        let mut result = analysis_with(serde_json::json!([
            {"message_id":"m1","quote":"시간대가 달라 키도 달라졌습니다."}
        ]));
        validate(&mut result, "s1", &catalog);
        let cache = crate::cache::TodayCache {
            date: "2026-10-31".into(),
            source_fingerprints: Default::default(),
            timezone_window_key: None,
            sources: vec![("Codex".into(), result)],
        };
        let saved = serde_json::to_string(&cache).unwrap();
        let loaded: crate::cache::TodayCache = serde_json::from_str(&saved).unwrap();
        let session = &loaded.sources[0].1.sessions[0];
        assert_eq!(session.til.len(), 1);
        assert_eq!(session.decisions[0].why, "");
        assert_eq!(
            session.til[0].evidence[0].location,
            Some(sources[0].location.clone())
        );
        let terminal = crate::output::display_evidence(&session.til[0].evidence);
        let md = crate::output::render_combined_markdown(
            &[("Codex", &loaded.sources[0].1)],
            chrono::NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
        );
        assert!(terminal.contains("/tmp/synthetic.jsonl:27 [m1]"));
        assert!(md.contains("/tmp/synthetic.jsonl:27 \\[m1\\]"));
        assert!(md.contains("시간대가 달라 키도 달라졌습니다."));
        assert!(md.contains("/tmp/synthetic.jsonl:27"));
        assert!(md.contains("USER"));
        assert!(!md.contains("근거 미확인"));
    }

    #[test]
    fn test_verified_quotes_resolve_local_location_and_drop_invented_insights() {
        let sources = vec![SourceMessage {
            timestamp: None,
            is_reference: false,
            message_index: 1,
            role: "USER".into(),
            text: "함수 하나로 충분합니다".into(),
            rendered_text: "함수 하나로 충분합니다".into(),
            location: SourceLocation::new(std::path::Path::new("/tmp/session.jsonl"), 3),
        }];
        let messages = vec![("USER".into(), "함수 하나로 충분합니다".into())];
        let (annotated, catalog) = annotate_messages(&messages, &sources);
        assert!(annotated[0].1.starts_with("[m1] "));
        let mut result = super::super::insight::parse_response(r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[{"what":"함수 사용","why":"간단함","evidence":[{"message_id":"m1","quote":"함수 하나로 충분합니다","location":{"path":"fake","line":99}}]},{"what":"만든 이유","why":"성능","evidence":[{"message_id":"m1","quote":"성능이 빠릅니다"}]}],"curiosities":[],"corrections":[],"til":[{"title":"없는 학습","detail":"추측","evidence":[]}]}]}"#).unwrap();
        validate(&mut result, "s1", &catalog);
        assert_eq!(result.sessions[0].decisions.len(), 1);
        assert!(result.sessions[0].til.is_empty());
        let proof = &result.sessions[0].decisions[0].evidence[0];
        assert_eq!(proof.location, Some(sources[0].location.clone()));
        assert_eq!(proof.role, "USER");
    }
}
