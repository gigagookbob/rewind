//! Bounded, file-backed reference speech from before the target day.

use super::{SessionTask, evidence::SourceMessage, prompt};
use crate::parser::UtcDateWindow;

pub const MAX_REFERENCE_MESSAGES: usize = 12;
pub const MAX_REFERENCE_CHARS: usize = 4_000;

/// The source keeps full speech locally, so a clipped secret's missing prefix
/// can still be detected before selecting the protected reference tail.
pub(super) fn render_protected_reference(source: &SourceMessage, protected: &str) -> String {
    let Some((marker, _)) = source
        .rendered_text
        .split_once("] ")
        .filter(|(marker, _)| marker.starts_with("[REFERENCE "))
    else {
        return protected.to_string();
    };
    let prefix = format!("{marker}] ");
    let room = source
        .rendered_text
        .chars()
        .count()
        .saturating_sub(prefix.chars().count());
    let visible = super::compaction::compact_log_like(protected);
    let tail: String = visible
        .chars()
        .skip(visible.chars().count().saturating_sub(room))
        .collect();
    format!("{prefix}{tail}")
}

fn select_reference(mut sources: Vec<SourceMessage>, window: UtcDateWindow) -> Vec<SourceMessage> {
    sources.retain(|s| s.timestamp.is_some_and(|ts| ts < window.start_utc));
    sources.sort_by_key(|s| s.timestamp);
    let mut selected = Vec::new();
    let mut remaining = MAX_REFERENCE_CHARS;
    for mut source in sources.into_iter().rev().take(MAX_REFERENCE_MESSAGES) {
        let Some(at) = source.timestamp else { continue };
        let prefix = format!("[REFERENCE {}; prior context only] ", at.to_rfc3339());
        let room = remaining.saturating_sub(prefix.chars().count());
        if room == 0 {
            break;
        }
        let visible = super::compaction::compact_log_like(&source.text);
        let count = visible.chars().count();
        let tail: String = visible.chars().skip(count.saturating_sub(room)).collect();
        source.rendered_text = format!("{prefix}{tail}");
        source.is_reference = true;
        remaining -= source.rendered_text.chars().count();
        selected.push(source);
    }
    selected.reverse();
    selected
}

/// Prepend only bounded visible speech. Today's message positions move together
/// with their source records, so IDs still address exactly one physical line.
pub fn prepend_reference(
    task: &mut SessionTask,
    sources: Vec<SourceMessage>,
    window: UtcDateWindow,
) {
    let mut references = select_reference(sources, window);
    if references.is_empty() {
        return;
    }
    let count = references.len();
    for source in &mut task.evidence_sources {
        source.message_index += count;
    }
    for (index, source) in references.iter_mut().enumerate() {
        source.message_index = index + 1;
    }
    let mut messages: Vec<_> = references
        .iter()
        .map(|s| (s.role.clone(), s.rendered_text.clone()))
        .collect();
    messages.append(&mut task.messages);
    references.append(&mut task.evidence_sources);
    task.evidence_sources = references;
    task.messages = messages;
    task.prompt_text = format!("[Session: {}]\n", task.session_id);
    for (role, text) in &task.messages {
        task.prompt_text.push_str(&format!("[{role}] {text}\n"));
    }
    task.estimated_tokens =
        prompt::estimate_prompt_tokens(&task.prompt_text) + task.messages.len() as u64 * 8;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(at: &str, text: &str) -> SourceMessage {
        SourceMessage {
            timestamp: Some(at.parse().unwrap()),
            is_reference: false,
            message_index: 1,
            role: "USER".into(),
            text: text.into(),
            rendered_text: text.into(),
            location: crate::parser::SourceLocation::new(std::path::Path::new("old.jsonl"), 4),
        }
    }

    #[test]
    fn test_reference_is_bounded_dated_and_does_not_include_today_or_future() {
        let window = UtcDateWindow {
            start_utc: "2026-10-31T15:00:00Z".parse().unwrap(),
            end_utc: "2026-11-01T15:00:00Z".parse().unwrap(),
        };
        let mut sources = vec![speech("2026-10-31T14:59:00Z", &"이전".repeat(5_000))];
        sources[0].rendered_text = "HIDDEN_THINKING".into();
        sources.push(speech("2026-10-31T15:00:00Z", "오늘"));
        sources.push(speech("2026-11-01T15:00:00Z", "미래"));
        let mut undated = speech("2026-10-31T14:59:00Z", "UNDATED");
        undated.timestamp = None;
        sources.push(undated);
        let selected = select_reference(sources, window);
        assert_eq!(selected.len(), 1);
        assert!(selected[0].is_reference);
        assert_eq!(
            selected[0].rendered_text.chars().count(),
            MAX_REFERENCE_CHARS
        );
        assert!(
            selected[0]
                .rendered_text
                .starts_with("[REFERENCE 2026-10-31T14:59:00+00:00")
        );
        assert_eq!(selected[0].location.line, 4);
        assert!(!selected[0].rendered_text.contains("오늘"));
        assert!(!selected[0].rendered_text.contains("미래"));
        assert!(!selected[0].rendered_text.contains("HIDDEN_THINKING"));
        assert!(!selected[0].rendered_text.contains("UNDATED"));
    }

    #[test]
    fn test_reference_keeps_latest_twelve_in_chronological_order() {
        let window = UtcDateWindow {
            start_utc: "2026-11-01T00:00:00Z".parse().unwrap(),
            end_utc: "2026-11-02T00:00:00Z".parse().unwrap(),
        };
        let sources = (0..20)
            .rev()
            .map(|n| speech(&format!("2026-10-31T12:{n:02}:00Z"), &format!("choice-{n}")))
            .collect();
        let selected = select_reference(sources, window);
        assert_eq!(selected.len(), 12);
        assert_eq!(selected.first().unwrap().text, "choice-8");
        assert_eq!(selected.last().unwrap().text, "choice-19");
    }
}
