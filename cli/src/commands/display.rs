//! Terminal rendering and session statistics used by the information box.

use super::collection::CollectedSource;
use crate::analyzer;

// ANSI color codes — visible on both light and dark terminals.
const CYAN: &str = "\x1b[36m";
const BRIGHT_BLUE: &str = "\x1b[94m";
pub(super) const YELLOW: &str = "\x1b[33m";
pub(super) const MAGENTA: &str = "\x1b[35m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
pub(super) const GREEN: &str = "\x1b[32m";
pub(super) const RESET: &str = "\x1b[0m";

/// Info box heading colors, assigned by adapter order (Claude Code, Codex, ...).
const SOURCE_HEADING_COLORS: &[&str] = &[BRIGHT_BLUE, YELLOW, MAGENTA, CYAN];

pub(super) fn format_roots_for_display(roots: &[std::path::PathBuf]) -> String {
    roots
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
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

/// One line of the info box.
#[derive(Debug, PartialEq, Eq)]
enum InfoRow {
    Text(String),
    Separator,
    Heading { color: &'static str, text: String },
}

/// Builds info box rows: the date, then one section per source.
///
/// Sources without any discovered log root get no section. Sources whose
/// roots exist but have no sessions on the date show "No sessions".
fn info_box_rows(date: chrono::NaiveDate, sources: &[CollectedSource]) -> Vec<InfoRow> {
    let mut rows = vec![InfoRow::Text(date.to_string())];

    for (index, source) in sources.iter().enumerate() {
        let collection = &source.collection;
        if collection.roots.is_empty() {
            continue;
        }
        rows.push(InfoRow::Separator);
        rows.push(InfoRow::Heading {
            color: SOURCE_HEADING_COLORS[index % SOURCE_HEADING_COLORS.len()],
            text: source.display_name.to_string(),
        });

        if collection.sessions.is_empty() {
            rows.push(InfoRow::Text(
                crate::messages::display::NO_SESSIONS.to_string(),
            ));
            continue;
        }

        let local = |ts: chrono::DateTime<chrono::Utc>| ts.with_timezone(&chrono::Local);
        let earliest = collection
            .sessions
            .iter()
            .filter_map(|s| s.first_activity)
            .min()
            .map(local);
        let latest = collection
            .sessions
            .iter()
            .filter_map(|s| s.last_activity)
            .max()
            .map(local);
        rows.push(InfoRow::Text(format_time_range(earliest, latest)));

        let token_totals = collection
            .sessions
            .iter()
            .filter_map(|s| s.token_usage)
            .reduce(|(acc_in, acc_out), (i, o)| (acc_in + i, acc_out + o));
        let count_line = match token_totals {
            Some((total_in, total_out)) => crate::messages::display::session_count_with_tokens(
                collection.sessions.len(),
                &format_number(total_in),
                &format_number(total_out),
            ),
            None => crate::messages::display::session_count(collection.sessions.len()),
        };
        rows.push(InfoRow::Text(count_line));
    }

    rows
}

/// Prints date and session summary as a Unicode box table.
/// Box width is dynamically sized to the longest content line.
pub(super) fn print_info_box(date: chrono::NaiveDate, sources: &[CollectedSource]) {
    let rows = info_box_rows(date, sources);

    // Determine box width from the longest content line (CJK chars count as 2).
    let content_max = rows
        .iter()
        .filter_map(|row| match row {
            InfoRow::Text(text) | InfoRow::Heading { text, .. } => {
                Some(unicode_display_width(text))
            }
            InfoRow::Separator => None,
        })
        .max()
        .unwrap_or(20);
    let w = content_max + 4;

    let line = "─".repeat(w);
    println!("\n  ┌{line}┐");

    for row in &rows {
        match row {
            InfoRow::Separator => println!("  ├{line}┤"),
            InfoRow::Heading { color, text } => {
                let pad = w - 2 - unicode_display_width(text);
                println!("  │  {color}{text}{RESET}{:pad$}│", "");
            }
            InfoRow::Text(text) => {
                let pad = w - 2 - unicode_display_width(text);
                println!("  │  {text}{:pad$}│", "");
            }
        }
    }

    println!("  └{line}┘");
}

/// Shows the first root and how many more were found, e.g. "/a (+2 more)".
fn format_roots_short(roots: &[std::path::PathBuf]) -> String {
    match roots {
        [] => String::new(),
        [only] => only.display().to_string(),
        [first, rest @ ..] => format!("{} (+{} more)", first.display(), rest.len()),
    }
}

/// Builds the `--dry-run` report shown after the info box: per-source
/// session rows, excluded counts, and a closing note. Only ids, times, and
/// counts are printed, never conversation text.
fn dry_run_lines(sources: &[CollectedSource]) -> Vec<String> {
    use crate::messages::dry_run;

    let mut lines = Vec::new();
    for source in sources {
        let collection = &source.collection;
        if collection.roots.is_empty() {
            lines.push(dry_run::no_roots(source.display_name));
            lines.push(String::new());
            continue;
        }
        lines.push(dry_run::source_heading(
            source.display_name,
            &format_roots_short(&collection.roots),
        ));

        let local = |ts: chrono::DateTime<chrono::Utc>| ts.with_timezone(&chrono::Local);
        for session in &collection.sessions {
            let task = &session.task;
            let count_role = |role: &str| task.messages.iter().filter(|(r, _)| r == role).count();
            let short_id: String = task.session_id.chars().take(8).collect();
            lines.push(dry_run::session_row(
                &short_id,
                &format_time_range(
                    session.first_activity.map(local),
                    session.last_activity.map(local),
                ),
                count_role("USER"),
                count_role("ASSISTANT"),
                &format_number(task.estimated_tokens),
            ));
        }

        let excluded = collection
            .excluded
            .iter()
            .map(|(reason, count)| dry_run::excluded_item(reason, *count))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(dry_run::source_summary(
            collection.sessions.len(),
            &excluded,
        ));
        lines.push(String::new());
    }

    let total = sources.iter().map(|s| s.collection.sessions.len()).sum();
    lines.push(dry_run::total(total));
    lines.push(dry_run::LLM_NOT_CALLED.to_string());
    lines
}

/// Prints the info box followed by the `--dry-run` session report.
pub(super) fn print_dry_run(date: chrono::NaiveDate, sources: &[CollectedSource]) {
    print_info_box(date, sources);
    println!();
    for line in dry_run_lines(sources) {
        println!("{line}");
    }
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
    use crate::analyzer::SessionTask;
    use crate::sources::{CollectedSession, SourceCollection};

    fn source(
        display_name: &'static str,
        roots: Vec<std::path::PathBuf>,
        sessions: Vec<CollectedSession>,
    ) -> CollectedSource {
        CollectedSource {
            id: display_name,
            display_name,
            collection: SourceCollection {
                roots,
                sessions,
                ..SourceCollection::default()
            },
        }
    }

    fn session(token_usage: Option<(u64, u64)>) -> CollectedSession {
        CollectedSession {
            task: SessionTask {
                source: "codex",
                session_id: "s1".to_string(),
                prompt_text: String::new(),
                messages: Vec::new(),
                estimated_tokens: 0,
            },
            first_activity: None,
            last_activity: None,
            token_usage,
        }
    }

    fn date() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date")
    }

    #[test]
    fn test_info_box_rows_show_no_sessions_when_root_exists_without_sessions() {
        let sources = vec![source("Claude Code", vec!["/claude".into()], Vec::new())];

        let rows = info_box_rows(date(), &sources);

        assert_eq!(
            rows,
            vec![
                InfoRow::Text("2099-01-01".to_string()),
                InfoRow::Separator,
                InfoRow::Heading {
                    color: BRIGHT_BLUE,
                    text: "Claude Code".to_string(),
                },
                InfoRow::Text(crate::messages::display::NO_SESSIONS.to_string()),
            ]
        );
    }

    #[test]
    fn test_info_box_rows_skip_sources_without_roots() {
        let sources = vec![
            source("Claude Code", Vec::new(), Vec::new()),
            source("Codex", vec!["/codex".into()], vec![session(None)]),
        ];

        let rows = info_box_rows(date(), &sources);

        let headings: Vec<&InfoRow> = rows
            .iter()
            .filter(|row| matches!(row, InfoRow::Heading { .. }))
            .collect();
        assert_eq!(
            headings,
            vec![&InfoRow::Heading {
                color: YELLOW,
                text: "Codex".to_string(),
            }]
        );
        assert!(rows.contains(&InfoRow::Text(crate::messages::display::session_count(1))));
    }

    #[test]
    fn test_dry_run_lines_hide_session_text_and_show_counts() {
        const MARKER: &str = "DRY_RUN_SECRET_MARKER";
        let mut source = source(
            "Claude Code",
            vec!["/claude-a".into(), "/claude-b".into()],
            vec![CollectedSession {
                task: SessionTask {
                    source: "claude",
                    session_id: "c0d5ead8-1234-5678".to_string(),
                    prompt_text: format!("[USER] {MARKER}\n"),
                    messages: vec![
                        ("USER".to_string(), MARKER.to_string()),
                        ("ASSISTANT".to_string(), MARKER.to_string()),
                        ("ASSISTANT".to_string(), "ok".to_string()),
                        ("SNAPSHOT".to_string(), "m1".to_string()),
                    ],
                    estimated_tokens: 41_200,
                },
                first_activity: None,
                last_activity: None,
                token_usage: None,
            }],
        );
        source.collection.excluded = vec![("automated", 2)];

        let lines = dry_run_lines(&[source]);
        let text = lines.join("\n");

        assert!(!text.contains(MARKER), "session text leaked: {text}");
        assert_eq!(
            lines,
            vec![
                crate::messages::dry_run::source_heading("Claude Code", "/claude-a (+1 more)"),
                crate::messages::dry_run::session_row("c0d5ead8", "? ~ ?", 1, 2, "41,200"),
                crate::messages::dry_run::source_summary(1, "2 automated"),
                String::new(),
                crate::messages::dry_run::total(1),
                crate::messages::dry_run::LLM_NOT_CALLED.to_string(),
            ]
        );
    }

    #[test]
    fn test_dry_run_lines_mark_sources_without_roots_or_sessions() {
        let mut codex = source("Codex", vec!["/codex".into()], Vec::new());
        codex.collection.excluded = vec![("subagent", 0)];
        let sources = vec![source("Claude Code", Vec::new(), Vec::new()), codex];

        let lines = dry_run_lines(&sources);

        assert_eq!(
            lines,
            vec![
                crate::messages::dry_run::no_roots("Claude Code"),
                String::new(),
                crate::messages::dry_run::source_heading("Codex", "/codex"),
                crate::messages::dry_run::source_summary(0, "0 subagent"),
                String::new(),
                crate::messages::dry_run::total(0),
                crate::messages::dry_run::LLM_NOT_CALLED.to_string(),
            ]
        );
    }

    #[test]
    fn test_info_box_rows_sum_token_usage_when_recorded() {
        let sources = vec![source(
            "Claude Code",
            vec!["/claude".into()],
            vec![session(Some((1_000, 20))), session(Some((234, 5)))],
        )];

        let rows = info_box_rows(date(), &sources);

        assert!(rows.contains(&InfoRow::Text(
            crate::messages::display::session_count_with_tokens(2, "1,234", "25")
        )));
        assert!(rows.contains(&InfoRow::Text("? ~ ?".to_string())));
    }
}
