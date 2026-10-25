//! Subagent task and result lines for the parent session prompt (SPEC.md,
//! stage 4).
//!
//! Subagents never count as sessions. Instead the parent prompt gets a
//! `[SUBAGENT]` line (who was asked to do what) and a `[SUBAGENT_RESULT]` line
//! (what came back) at the point in time the parent received them.

use chrono::{DateTime, NaiveDate, Utc};

use crate::analyzer::compaction::compact_log_like;
use crate::analyzer::prompt::TimedLine;

/// Per-result cap chosen from 72 measured subagent results: 6 of them go
/// over it (SPEC.md, stage 4 length caps).
pub const SUBAGENT_RESULT_MAX_CHARS: usize = 8_000;
/// Results budget per parent session, about 12,000 estimated tokens. Chosen
/// because one measured session called 26 subagents (SPEC.md, stage 4).
pub const SUBAGENT_SESSION_MAX_CHARS: usize = 24_000;
/// Per-task cap. Measured: Claude workflow tasks reach 99,073 chars and 23 of
/// 63 tasks go over 2,000 (SPEC.md, stage 4 length caps).
pub const SUBAGENT_TASK_MAX_CHARS: usize = 2_000;
/// Tasks budget per parent session. Measured: one parent's tasks summed to
/// 168,530 chars (SPEC.md, stage 4 length caps).
pub const SUBAGENT_TASK_SESSION_MAX_CHARS: usize = 6_000;

/// Prompt role for "<name>: <task>".
pub const ROLE_SUBAGENT: &str = "SUBAGENT";
/// Prompt role for the subagent's final answer.
pub const ROLE_SUBAGENT_RESULT: &str = "SUBAGENT_RESULT";

/// One subagent run as its parent session saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubagentRun {
    /// Agent type or nickname, e.g. "Explore", "workflow", or "Ada".
    pub name: String,
    /// What the parent asked for; may be empty.
    pub task: String,
    /// The final answer; empty when the subagent wrote none.
    pub result: String,
    /// Where the lines go: before the first parent message after this time.
    pub at: DateTime<Utc>,
}

/// Keeps runs whose position falls inside the target local date.
pub fn keep_target_date(runs: Vec<SubagentRun>, date: NaiveDate) -> Vec<SubagentRun> {
    let window = crate::parser::local_date_to_utc_window(date).ok();
    runs.into_iter()
        .filter(|run| match window {
            Some(window) => window.contains(run.at),
            // Fallback if local midnight resolution fails unexpectedly.
            None => run.at.with_timezone(&chrono::Local).date_naive() == date,
        })
        .collect()
}

/// Turns one parent session's runs into prompt lines, in time order.
///
/// Both texts go through `compact_log_like`, then tasks and results are capped
/// against their own per-item and per-session budgets.
pub fn subagent_lines(runs: &[SubagentRun]) -> Vec<TimedLine> {
    let mut runs: Vec<&SubagentRun> = runs.iter().collect();
    runs.sort_by_key(|run| run.at);

    let tasks = cap_to_budget(
        runs.iter()
            .map(|run| compact_log_like(run.task.trim()))
            .collect(),
        SUBAGENT_TASK_MAX_CHARS,
        SUBAGENT_TASK_SESSION_MAX_CHARS,
    );
    let results = cap_to_budget(
        runs.iter()
            .map(|run| compact_log_like(run.result.trim()))
            .collect(),
        SUBAGENT_RESULT_MAX_CHARS,
        SUBAGENT_SESSION_MAX_CHARS,
    );

    let mut lines = Vec::with_capacity(runs.len() * 2);
    for ((run, task), result) in runs.into_iter().zip(tasks).zip(results) {
        let text = if task.is_empty() {
            run.name.clone()
        } else {
            format!("{}: {task}", run.name)
        };
        lines.push(TimedLine {
            at: run.at,
            role: ROLE_SUBAGENT,
            text,
        });
        if !result.is_empty() {
            lines.push(TimedLine {
                at: run.at,
                role: ROLE_SUBAGENT_RESULT,
                text: result,
            });
        }
    }
    lines
}

/// Caps every text to `per_item` chars. When that still exceeds
/// `session_total`, every text gets `min(per_item, session_total / n)` chars,
/// where `n` counts non-empty texts.
fn cap_to_budget(texts: Vec<String>, per_item: usize, session_total: usize) -> Vec<String> {
    let lengths: Vec<usize> = texts
        .iter()
        .map(|text| text.chars().count())
        .filter(|&len| len > 0)
        .collect();
    let capped_total: usize = lengths.iter().map(|&len| len.min(per_item)).sum();
    let share = if capped_total <= session_total {
        per_item
    } else {
        per_item.min(session_total / lengths.len())
    };
    texts
        .into_iter()
        .map(|text| truncate_middle(&text, share))
        .collect()
}

/// Shortens `text` to at most `max_chars` chars (marker included), keeping the
/// first two thirds and the last third. Reports often end with the
/// conclusion, so the tail is kept; the 2:1 split is a judgment call, not
/// measured (SPEC.md, stage 4 length caps).
pub fn truncate_middle(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    // The marker for `total` omitted chars is the widest one possible.
    let widest_marker = omission_marker(total).chars().count();
    let keep = max_chars.saturating_sub(widest_marker);
    if keep == 0 {
        return text.chars().take(max_chars).collect();
    }
    let head = keep * 2 / 3;
    let tail = keep - head;
    let mut out: String = text.chars().take(head).collect();
    out.push_str(&omission_marker(total - keep));
    out.extend(text.chars().skip(total - tail));
    out
}

fn omission_marker(omitted: usize) -> String {
    format!("[... {omitted}자 생략 ...]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        s.parse().expect("timestamp")
    }

    fn run(name: &str, task: &str, result: &str, ts: &str) -> SubagentRun {
        SubagentRun {
            name: name.to_string(),
            task: task.to_string(),
            result: result.to_string(),
            at: at(ts),
        }
    }

    fn line_texts<'a>(lines: &'a [TimedLine], role: &str) -> Vec<&'a str> {
        lines
            .iter()
            .filter(|line| line.role == role)
            .map(|line| line.text.as_str())
            .collect()
    }

    #[test]
    fn test_truncate_middle_short_text_unchanged() {
        assert_eq!(truncate_middle("short", 8_000), "short");
        let exact = "x".repeat(8_000);
        assert_eq!(truncate_middle(&exact, 8_000), exact);
    }

    #[test]
    fn test_truncate_middle_keeps_head_two_thirds_and_tail_third() {
        let text: String = (0..9_000)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        let out = truncate_middle(&text, SUBAGENT_RESULT_MAX_CHARS);

        assert_eq!(out.chars().count(), SUBAGENT_RESULT_MAX_CHARS);
        // 18-char marker leaves 7,982 chars: 5,321 from the head, 2,661 from the tail.
        let marker = "[... 1018자 생략 ...]";
        let (head, tail) = out.split_once(marker).expect("marker");
        assert_eq!(head, &text[..5_321]);
        assert_eq!(tail, &text[9_000 - 2_661..]);
    }

    #[test]
    fn test_truncate_middle_counts_chars_not_bytes() {
        let text = "가".repeat(3_000);
        let out = truncate_middle(&text, SUBAGENT_TASK_MAX_CHARS);
        assert_eq!(out.chars().count(), SUBAGENT_TASK_MAX_CHARS);
        assert!(out.contains("자 생략 ...]"));
    }

    #[test]
    fn test_truncate_middle_without_room_for_marker_keeps_head_only() {
        assert_eq!(truncate_middle(&"y".repeat(100), 5), "yyyyy");
    }

    #[test]
    fn test_subagent_lines_emit_task_and_result_lines() {
        let lines = subagent_lines(&[run("Explore", "find x", "found x", "2099-01-01T10:00:00Z")]);
        assert_eq!(
            lines,
            vec![
                TimedLine {
                    at: at("2099-01-01T10:00:00Z"),
                    role: ROLE_SUBAGENT,
                    text: "Explore: find x".to_string(),
                },
                TimedLine {
                    at: at("2099-01-01T10:00:00Z"),
                    role: ROLE_SUBAGENT_RESULT,
                    text: "found x".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_subagent_lines_without_result_emit_task_line_only() {
        let lines = subagent_lines(&[
            run("workflow", "step", "  ", "2099-01-01T10:00:00Z"),
            run("subagent", "", "done", "2099-01-01T10:01:00Z"),
        ]);
        let roles: Vec<&str> = lines.iter().map(|line| line.role).collect();
        assert_eq!(
            roles,
            vec![ROLE_SUBAGENT, ROLE_SUBAGENT, ROLE_SUBAGENT_RESULT]
        );
        assert_eq!(
            line_texts(&lines, ROLE_SUBAGENT),
            vec!["workflow: step", "subagent"]
        );
    }

    #[test]
    fn test_subagent_lines_sorted_by_time() {
        let lines = subagent_lines(&[
            run("b", "second", "", "2099-01-01T11:00:00Z"),
            run("a", "first", "", "2099-01-01T10:00:00Z"),
        ]);
        assert_eq!(
            line_texts(&lines, ROLE_SUBAGENT),
            vec!["a: first", "b: second"]
        );
    }

    #[test]
    fn test_subagent_lines_cap_each_result_at_8000_when_budget_fits() {
        let long = "r".repeat(9_000);
        let lines = subagent_lines(&[
            run("a", "t", &long, "2099-01-01T10:00:00Z"),
            run("b", "t", &long, "2099-01-01T10:01:00Z"),
        ]);
        for text in line_texts(&lines, ROLE_SUBAGENT_RESULT) {
            assert_eq!(text.chars().count(), SUBAGENT_RESULT_MAX_CHARS);
        }
    }

    #[test]
    fn test_subagent_lines_shrink_result_share_over_session_budget() {
        let long = "r".repeat(7_000);
        let runs: Vec<SubagentRun> = (0..5)
            .map(|i| run("a", "t", &long, &format!("2099-01-01T10:0{i}:00Z")))
            .collect();
        let lines = subagent_lines(&runs);

        let results = line_texts(&lines, ROLE_SUBAGENT_RESULT);
        assert_eq!(results.len(), 5);
        // Capped total 35,000 > 24,000, so each share is min(8,000, 24,000 / 5).
        for text in &results {
            assert_eq!(text.chars().count(), 4_800);
        }
        let total: usize = results.iter().map(|t| t.chars().count()).sum();
        assert!(total <= SUBAGENT_SESSION_MAX_CHARS);
    }

    #[test]
    fn test_subagent_lines_cap_tasks_per_item_and_per_session() {
        let long_task = "t".repeat(5_000);
        let runs: Vec<SubagentRun> = (0..4)
            .map(|i| run("a", &long_task, "ok", &format!("2099-01-01T10:0{i}:00Z")))
            .collect();
        let lines = subagent_lines(&runs);

        let tasks = line_texts(&lines, ROLE_SUBAGENT);
        // Capped total 8,000 > 6,000, so each task share is min(2,000, 6,000 / 4).
        for text in &tasks {
            let task = text.strip_prefix("a: ").expect("name prefix");
            assert_eq!(task.chars().count(), 1_500);
        }
        let single = subagent_lines(&[run("a", &long_task, "ok", "2099-01-01T10:00:00Z")]);
        let task = line_texts(&single, ROLE_SUBAGENT)[0]
            .strip_prefix("a: ")
            .expect("name prefix");
        assert_eq!(task.chars().count(), SUBAGENT_TASK_MAX_CHARS);
    }

    #[test]
    fn test_subagent_lines_empty_input_yields_no_lines() {
        assert!(subagent_lines(&[]).is_empty());
    }

    #[test]
    fn test_keep_target_date_drops_runs_outside_local_day() {
        use chrono::TimeZone;
        let local = |day: u32, hour: u32| {
            chrono::Local
                .with_ymd_and_hms(2099, 1, day, hour, 0, 0)
                .earliest()
                .expect("local time")
                .with_timezone(&Utc)
        };
        let mut inside = run("a", "t", "r", "2099-01-01T00:00:00Z");
        inside.at = local(2, 12);
        let mut before = inside.clone();
        before.at = local(1, 23);
        let mut after = inside.clone();
        after.at = local(3, 0);

        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 2).expect("date");
        let kept = keep_target_date(vec![before, inside.clone(), after], date);
        assert_eq!(kept, vec![inside]);
    }
}
