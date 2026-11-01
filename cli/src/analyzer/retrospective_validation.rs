//! Conservative structural checks; quotation meaning still depends on extraction.
use super::evidence::{self, SourceMessage};
use super::insight::{Evidence, SessionInsight};
use super::retrospective::{FollowUpOrigin, FollowUpUpdateKind, QuestionUpdateKind};
use std::collections::HashMap;

pub(super) fn validate(
    session: &mut SessionInsight,
    catalog: &HashMap<String, SourceMessage>,
    cached: bool,
) {
    let check = |proofs: &mut [Evidence]| {
        if cached {
            evidence::cached_evidence_matches(proofs, catalog)
        } else {
            evidence::resolve(proofs, catalog)
        }
    };
    if session.task_title.as_mut().is_some_and(|title| {
        title.text.trim().is_empty()
            || !check(&mut title.evidence)
            || !role_matches(&title.evidence, "USER")
            || !has_today(&title.evidence)
    }) {
        session.task_title = None;
    }
    session.questions.retain_mut(|question| {
        if question.question.trim().is_empty()
            || !check(&mut question.evidence)
            || !role_matches(&question.evidence, "USER")
        {
            return false;
        }
        let mut last = latest(&question.evidence, catalog);
        let mut today = has_today(&question.evidence);
        for update in &mut question.updates {
            let role = match update.kind {
                QuestionUpdateKind::Answer => "ASSISTANT",
                QuestionUpdateKind::Resolved | QuestionUpdateKind::Open => "USER",
                QuestionUpdateKind::Unknown => return false,
            };
            if update.detail.trim().is_empty()
                || !check(&mut update.evidence)
                || !role_matches(&update.evidence, role)
                || !strictly_after(&update.evidence, last, catalog)
            {
                return false;
            }
            last = latest(&update.evidence, catalog);
            today |= has_today(&update.evidence);
        }
        today
    });
    session.follow_ups.retain_mut(|task| {
        let role = match task.origin {
            FollowUpOrigin::UserRequest => "USER",
            FollowUpOrigin::AiSuggestion => "ASSISTANT",
            FollowUpOrigin::Unknown => return false,
        };
        if task.task.trim().is_empty()
            || !check(&mut task.evidence)
            || !role_matches(&task.evidence, role)
        {
            return false;
        }
        let mut last = latest(&task.evidence, catalog);
        let mut today = has_today(&task.evidence);
        let mut terminal = false;
        let mut withdrawn = false;
        for update in &mut task.updates {
            let role = match update.kind {
                FollowUpUpdateKind::Accepted if !terminal => "USER",
                FollowUpUpdateKind::Completed if !withdrawn => "",
                FollowUpUpdateKind::Withdrawn => "USER",
                _ => return false,
            };
            if update.detail.trim().is_empty()
                || !check(&mut update.evidence)
                || !role_matches(&update.evidence, role)
                || !strictly_after(&update.evidence, last, catalog)
            {
                return false;
            }
            last = latest(&update.evidence, catalog);
            today |= has_today(&update.evidence);
            withdrawn |= update.kind == FollowUpUpdateKind::Withdrawn;
            terminal |= matches!(
                update.kind,
                FollowUpUpdateKind::Completed | FollowUpUpdateKind::Withdrawn
            );
        }
        today
    });
}
fn role_matches(proofs: &[Evidence], role: &str) -> bool {
    !proofs.is_empty()
        && proofs.iter().all(|p| {
            if role.is_empty() {
                p.role == "USER" || p.role == "ASSISTANT"
            } else {
                p.role == role
            }
        })
}
fn has_today(proofs: &[Evidence]) -> bool {
    proofs.iter().any(|p| !p.is_reference)
}
fn latest(proofs: &[Evidence], catalog: &HashMap<String, SourceMessage>) -> usize {
    proofs
        .iter()
        .filter_map(|p| catalog.get(&p.message_id))
        .map(|s| s.message_index)
        .max()
        .unwrap_or(0)
}
fn strictly_after(
    proofs: &[Evidence],
    index: usize,
    catalog: &HashMap<String, SourceMessage>,
) -> bool {
    proofs.iter().all(|p| {
        catalog
            .get(&p.message_id)
            .is_some_and(|s| s.message_index > index)
    })
}

/// Every origin and update must be visible in the selected source fragments.
/// Drop the whole chain on failure; dropping only a withdrawal would revive work.
pub(super) fn retain_supported(
    session: &mut SessionInsight,
    supported: impl Fn(&[Evidence]) -> bool,
) {
    if session
        .task_title
        .as_ref()
        .is_some_and(|t| !supported(&t.evidence))
    {
        session.task_title = None;
    }
    session
        .questions
        .retain(|q| supported(&q.evidence) && q.updates.iter().all(|u| supported(&u.evidence)));
    session
        .follow_ups
        .retain(|t| supported(&t.evidence) && t.updates.iter().all(|u| supported(&u.evidence)));
}
