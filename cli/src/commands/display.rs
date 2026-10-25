//! Terminal rendering and session statistics used by the information box.

use super::collection::CodexCollectedSession;
use crate::{analyzer, parser};

// ANSI color codes — visible on both light and dark terminals.
const CYAN: &str = "\x1b[36m";
const BRIGHT_BLUE: &str = "\x1b[94m";
pub(super) const YELLOW: &str = "\x1b[33m";
pub(super) const MAGENTA: &str = "\x1b[35m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
pub(super) const GREEN: &str = "\x1b[32m";
pub(super) const RESET: &str = "\x1b[0m";

pub(super) fn format_roots_for_display(roots: &[std::path::PathBuf]) -> String {
    roots
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Returns the earliest local timestamp from Claude entries.
fn claude_earliest_time(
    entries: &[parser::claude::LogEntry],
) -> Option<chrono::DateTime<chrono::Local>> {
    entries
        .iter()
        .filter_map(parser::claude::entry_timestamp)
        .min()
        .map(|ts| ts.with_timezone(&chrono::Local))
}

/// Returns the latest local timestamp from Claude entries.
fn claude_latest_time(
    entries: &[parser::claude::LogEntry],
) -> Option<chrono::DateTime<chrono::Local>> {
    entries
        .iter()
        .filter_map(parser::claude::entry_timestamp)
        .max()
        .map(|ts| ts.with_timezone(&chrono::Local))
}

/// Returns the earliest local timestamp from Codex sessions.
fn codex_earliest_time(
    sessions: &[CodexCollectedSession],
) -> Option<chrono::DateTime<chrono::Local>> {
    codex_activity_timestamps(sessions)
        .min()
        .map(|ts| ts.with_timezone(&chrono::Local))
}

/// Returns the latest local timestamp from Codex sessions.
fn codex_latest_time(
    sessions: &[CodexCollectedSession],
) -> Option<chrono::DateTime<chrono::Local>> {
    codex_activity_timestamps(sessions)
        .max()
        .map(|ts| ts.with_timezone(&chrono::Local))
}

/// Returns UTC timestamps for Codex activity entries (excluding SessionMeta).
fn codex_activity_timestamps(
    sessions: &[CodexCollectedSession],
) -> impl Iterator<Item = chrono::DateTime<chrono::Utc>> + '_ {
    sessions
        .iter()
        .flat_map(|(_, entries)| entries.iter())
        .filter_map(|e| match e {
            parser::codex::CodexEntry::UserMessage { timestamp, .. }
            | parser::codex::CodexEntry::AssistantMessage { timestamp, .. }
            | parser::codex::CodexEntry::FunctionCall { timestamp, .. } => Some(*timestamp),
            parser::codex::CodexEntry::SessionMeta { .. }
            | parser::codex::CodexEntry::Other { .. } => None,
        })
}

/// Computes total token counts from Claude session summaries: (total_in, total_out).
fn claude_total_tokens(summaries: &[parser::claude::SessionSummary]) -> (u64, u64) {
    summaries.iter().fold((0, 0), |(acc_in, acc_out), s| {
        let total_in =
            s.total_input_tokens + s.total_cache_creation_tokens + s.total_cache_read_tokens;
        (acc_in + total_in, acc_out + s.total_output_tokens)
    })
}

/// Formats a time range as "HH:MM ~ HH:MM".
fn format_time_range(
    earliest: Option<chrono::DateTime<chrono::Local>>,
    latest: Option<chrono::DateTime<chrono::Local>>,
) -> String {
    match (earliest, latest) {
        (Some(start), Some(end)) => format!("{} ~ {}", start.format("%H:%M"), end.format("%H:%M")),
        (Some(start), None) => format!("{} ~ ?", start.format("%H:%M")),
        (None, Some(end)) => format!("? ~ {}", end.format("%H:%M")),
        (None, None) => "? ~ ?".to_string(),
    }
}

/// Formats a number with thousands separators.
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Prints the RWD block ASCII logo banner.
pub(super) fn print_logo_banner() {
    let version = env!("CARGO_PKG_VERSION");
    println!();
    println!("{CYAN}  ██████  ██     ██ ██████{RESET}");
    println!("{CYAN}  ██   ██ ██     ██ ██   ██{RESET}");
    println!(
        "{CYAN}  ██████  ██  █  ██ ██   ██{RESET}     {DIM}rewind your day  v{version}{RESET}"
    );
    println!("{CYAN}  ██   ██ ██ ███ ██ ██   ██{RESET}");
    println!("{CYAN}  ██   ██  ███ ███  ██████{RESET}");
}

/// Prints date and session summary as a Unicode box table.
/// Box width is dynamically sized to the longest content line.
pub(super) fn print_info_box(
    date: chrono::NaiveDate,
    claude_summaries: Option<&[parser::claude::SessionSummary]>,
    claude_entries: &[parser::claude::LogEntry],
    codex_sessions: &[CodexCollectedSession],
) {
    // Build rows as (color_kind, text) pairs. "sep" means separator line.
    let date_str = format!("{date}");

    let mut rows: Vec<(&str, String)> = Vec::new();
    rows.push(("plain", date_str));

    // Claude Code section
    if let Some(summaries) = claude_summaries {
        rows.push(("sep", String::new()));
        rows.push(("blue", "Claude Code".to_string()));

        let earliest = claude_earliest_time(claude_entries);
        let latest = claude_latest_time(claude_entries);
        let time_range = format_time_range(earliest, latest);
        rows.push(("plain", time_range));

        let (total_in, total_out) = claude_total_tokens(summaries);
        rows.push((
            "plain",
            crate::messages::display::session_count_with_tokens(
                summaries.len(),
                &format_number(total_in),
                &format_number(total_out),
            ),
        ));
    }

    // Codex section
    rows.push(("sep", String::new()));
    rows.push(("yellow", "Codex".to_string()));
    if !codex_sessions.is_empty() {
        let earliest = codex_earliest_time(codex_sessions);
        let latest = codex_latest_time(codex_sessions);
        let time_range = format_time_range(earliest, latest);
        rows.push(("plain", time_range));
        rows.push((
            "plain",
            crate::messages::display::session_count(codex_sessions.len()),
        ));
    } else {
        rows.push(("plain", crate::messages::display::NO_SESSIONS.to_string()));
    }

    // Determine box width from the longest content line (CJK chars count as 2).
    let content_max = rows
        .iter()
        .filter(|(kind, _)| *kind != "sep")
        .map(|(_, text)| unicode_display_width(text))
        .max()
        .unwrap_or(20);
    let w = content_max + 4;

    let line = "─".repeat(w);
    println!("\n  ┌{line}┐");

    for (kind, text) in &rows {
        match *kind {
            "sep" => println!("  ├{line}┤"),
            "blue" => {
                let pad = w - 2 - unicode_display_width(text);
                println!("  │  {BRIGHT_BLUE}{text}{RESET}{:pad$}│", "");
            }
            "yellow" => {
                let pad = w - 2 - unicode_display_width(text);
                println!("  │  {YELLOW}{text}{RESET}{:pad$}│", "");
            }
            _ => {
                let pad = w - 2 - unicode_display_width(text);
                println!("  │  {text}{:pad$}│", "");
            }
        }
    }

    println!("  └{line}┘");
}

/// Returns the terminal width by querying `stty size` via /dev/tty.
fn terminal_width() -> usize {
    if let Ok(tty) = std::fs::File::open("/dev/tty")
        && let Ok(output) = std::process::Command::new("stty")
            .arg("size")
            .stdin(tty)
            .output()
    {
        let s = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = s.split_whitespace().collect();
        if let Some(cols) = parts.get(1).and_then(|c| c.parse::<usize>().ok())
            && cols > 0
        {
            return cols;
        }
    }
    80
}

/// Returns the display width of a string (ASCII = 1, CJK = 2).
fn unicode_display_width(s: &str) -> usize {
    s.chars().map(|c| if c.is_ascii() { 1 } else { 2 }).sum()
}

/// Prints analysis results in a full-box style per source.
pub(super) fn print_insights(source_name: &str, analysis: &analyzer::AnalysisResult) {
    let term_w = terminal_width();
    let header_used = 5 + unicode_display_width(source_name) + 1;
    let line_len = if term_w > header_used {
        term_w - header_used
    } else {
        20
    };
    let line = "─".repeat(line_len);
    println!("\n{CYAN}  ┌─ {source_name} {line}{RESET}");

    for session in &analysis.sessions {
        // Show only the first 8 chars of session ID for readability
        let id_short = if session.session_id.len() >= 8 {
            &session.session_id[..8]
        } else {
            &session.session_id
        };
        println!("\n{BRIGHT_BLUE}  ▸ Session: {id_short}{RESET}");
        println!(
            "{}",
            crate::messages::display::summary_line(&session.work_summary)
        );

        if !session.decisions.is_empty() {
            println!(
                "\n  {YELLOW}{}{RESET}",
                crate::messages::display::DECISIONS_LABEL
            );
            for d in &session.decisions {
                println!("  • {}", d.what);
                println!("    {DIM}→ {}{RESET}", d.why);
            }
        }

        if !session.curiosities.is_empty() {
            println!(
                "\n  {YELLOW}{}{RESET}",
                crate::messages::display::CURIOSITIES_LABEL
            );
            for c in &session.curiosities {
                println!("  • {c}");
            }
        }

        if !session.corrections.is_empty() {
            println!(
                "\n  {YELLOW}{}{RESET}",
                crate::messages::display::CORRECTIONS_LABEL
            );
            for c in &session.corrections {
                println!("  {RED}\u{2717} {}{RESET}", c.model_said);
                println!("  {GREEN}\u{2713} {}{RESET}", c.user_corrected);
            }
        }
    }

    let bottom_line = "─".repeat(if term_w > 2 { term_w - 2 } else { 20 });
    println!("\n{CYAN}  └{bottom_line}{RESET}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_codex_time_range_ignores_previous_day_session_meta() {
        let session_meta_ts = chrono::Local
            .with_ymd_and_hms(2099, 1, 1, 23, 28, 0)
            .earliest()
            .expect("session meta local time")
            .with_timezone(&chrono::Utc);
        let first_activity_ts = chrono::Local
            .with_ymd_and_hms(2099, 1, 2, 0, 10, 0)
            .earliest()
            .expect("first activity local time")
            .with_timezone(&chrono::Utc);
        let last_activity_ts = chrono::Local
            .with_ymd_and_hms(2099, 1, 2, 20, 44, 0)
            .earliest()
            .expect("last activity local time")
            .with_timezone(&chrono::Utc);

        let summary = parser::codex::CodexSessionSummary {
            session_id: "s1".to_string(),
            cwd: "/tmp".to_string(),
            model_provider: "openai".to_string(),
            user_count: 1,
            assistant_count: 1,
            function_call_count: 0,
        };
        let sessions = vec![(
            summary,
            vec![
                parser::codex::CodexEntry::SessionMeta {
                    timestamp: session_meta_ts,
                    session_id: "s1".to_string(),
                    cwd: "/tmp".to_string(),
                    model_provider: "openai".to_string(),
                    subagent_source: None,
                    agent_role: None,
                    agent_nickname: None,
                    text: "s1\n/tmp\nopenai".to_string(),
                },
                parser::codex::CodexEntry::UserMessage {
                    timestamp: first_activity_ts,
                    text: "hello".to_string(),
                },
                parser::codex::CodexEntry::AssistantMessage {
                    timestamp: last_activity_ts,
                    text: "done".to_string(),
                },
            ],
        )];

        let earliest = codex_earliest_time(&sessions).expect("earliest");
        let latest = codex_latest_time(&sessions).expect("latest");

        assert_eq!(earliest.format("%H:%M").to_string(), "00:10");
        assert_eq!(latest.format("%H:%M").to_string(), "20:44");
    }
}
