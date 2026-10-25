//! Summary and Slack text workflows built from cached daily analyses.

use super::collection::{claude_include_automated, collect_claude_entries, collect_codex_sessions};
use super::display::{RESET, YELLOW};
use super::{resolve_lang, run_today};
use crate::{analyzer, cache, config, output};

/// Loads today's analysis cache, running the full analysis first when the
/// cache is missing or stale for the current timezone rules.
async fn load_cache_or_analyze(
    today: chrono::NaiveDate,
) -> Result<cache::TodayCache, Box<dyn std::error::Error>> {
    let cached = match cache::load_cache(today) {
        Some(c) => c,
        None => {
            println!("{}", crate::messages::error::NO_CACHE);
            run_today(false, None, today, false, false).await?;
            reload_cache_after_analysis(today)
        }
    };

    if cache::is_timezone_compatible(&cached, today) {
        return Ok(cached);
    }

    eprintln!(
        "{YELLOW}{}{RESET}",
        crate::messages::status::CACHE_STALE_TIMEZONE
    );
    eprintln!("{}", crate::messages::status::CACHE_STALE_HINT);
    eprintln!();
    run_today(false, None, today, false, false).await?;
    Ok(reload_cache_after_analysis(today))
}

/// Reloads the cache written by `run_today`. Exits when analysis produced none.
fn reload_cache_after_analysis(today: chrono::NaiveDate) -> cache::TodayCache {
    match cache::load_cache(today) {
        Some(c) => c,
        None => {
            eprintln!("{}", crate::messages::error::NO_CACHE_AFTER_ANALYSIS);
            std::process::exit(1);
        }
    }
}

/// Concatenates all session work_summaries with source names for project-level grouping.
fn collect_work_summaries(cached: &cache::TodayCache) -> String {
    let mut summaries_text = String::new();
    for (source_name, analysis) in &cached.sources {
        for session in &analysis.sessions {
            summaries_text.push_str(&format!(
                "[{source_name} / {}] {}\n",
                session.session_id, session.work_summary
            ));
        }
    }
    summaries_text
}

/// Generates a development progress summary for today.
///
/// 1. Loads today's cache (runs `run_today()` first if missing).
/// 2. Collects work_summary from all sessions and sends to LLM.
/// 3. Prints summary to terminal, appends to daily Markdown, copies to clipboard.
pub(crate) async fn run_summary(
    lang_flag: Option<String>,
    target_date: chrono::NaiveDate,
    no_cache: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let today = target_date;

    if no_cache {
        run_today(false, None, today, true, false).await?;
    }

    let cached = load_cache_or_analyze(today).await?;

    let summaries_text = collect_work_summaries(&cached);
    if summaries_text.is_empty() {
        println!("{}", crate::messages::error::NO_SESSIONS);
        return Ok(());
    }

    println!("{}", crate::messages::status::SUMMARY_GENERATING);
    let mut loaded_config = config::load_config_if_exists();
    let lang = resolve_lang(&lang_flag, &mut loaded_config).unwrap_or(config::Lang::En);
    let summary = analyzer::analyze_summary(&summaries_text, &lang).await?;

    println!("\n{}", crate::messages::status::SUMMARY_HEADER);
    println!("{summary}");

    // Append summary section to daily Markdown file.
    append_summary_to_markdown(today, &summary);

    copy_to_clipboard(&summary);
    println!("\n{}", crate::messages::status::COPIED_TO_CLIPBOARD);

    Ok(())
}

/// Generates a Slack-ready message from today's analysis.
///
/// Similar to `run_summary()` but uses SLACK_PROMPT for Slack-friendly formatting.
/// Only outputs to terminal and copies to clipboard (no Obsidian save).
pub(crate) async fn run_slack(
    lang_flag: Option<String>,
    target_date: chrono::NaiveDate,
    no_cache: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let today = target_date;

    if no_cache {
        run_today(false, None, today, true, false).await?;
    }

    let cached = load_cache_or_analyze(today).await?;

    // Warn if cache is stale (entry count mismatch).
    let mut loaded_config = config::load_config_if_exists();
    let include_automated = claude_include_automated(loaded_config.as_ref());
    let claude_count =
        collect_claude_entries(today, loaded_config.as_ref(), include_automated).len();
    let (codex_sessions, _) = collect_codex_sessions(today, loaded_config.as_ref());
    let codex_session_count = codex_sessions.len();
    let codex_entry_count: usize = codex_sessions
        .iter()
        .map(|(_, entries)| entries.len())
        .sum();
    if cached.claude_entry_count != claude_count
        || cached.codex_session_count != codex_session_count
        || cached.codex_entry_count != codex_entry_count
    {
        let cached_total = cached.claude_entry_count + cached.codex_entry_count;
        let current_total = claude_count + codex_entry_count;
        eprintln!(
            "{YELLOW}{}{RESET}",
            crate::messages::status::cache_stale(cached_total, current_total)
        );
        eprintln!("{}", crate::messages::status::CACHE_STALE_HINT);
        eprintln!();
    }

    let summaries_text = collect_work_summaries(&cached);
    if summaries_text.is_empty() {
        println!("{}", crate::messages::error::NO_SESSIONS);
        return Ok(());
    }

    println!("{}", crate::messages::status::SLACK_GENERATING);
    let lang = resolve_lang(&lang_flag, &mut loaded_config).unwrap_or(config::Lang::En);
    let slack_message = analyzer::analyze_slack(&summaries_text, &lang).await?;

    println!("\n{slack_message}");

    copy_to_clipboard(&slack_message);
    println!("\n{}", crate::messages::status::COPIED_TO_CLIPBOARD);

    Ok(())
}

/// Copies text to the system clipboard. macOS: pbcopy, Linux: xclip.
fn copy_to_clipboard(text: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = if cfg!(target_os = "macos") {
        Command::new("pbcopy")
    } else {
        let mut c = Command::new("xclip");
        c.arg("-selection").arg("clipboard");
        c
    };

    if let Ok(mut child) = cmd.stdin(Stdio::piped()).spawn() {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

/// Overwrites or appends the progress section in the daily Markdown file.
///
/// If the section already exists, replaces it in-place. Otherwise appends at the end.
/// Section range: from the header to the next `## ` header (or EOF).
fn append_summary_to_markdown(date: chrono::NaiveDate, summary: &str) {
    let vault_path = match output::load_vault_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", crate::messages::error::vault_path_load_failed(&e));
            return;
        }
    };

    let file_path = vault_path.join(format!("{date}.md"));
    if !file_path.exists() {
        eprintln!(
            "{}",
            crate::messages::error::daily_markdown_not_found(&file_path.display())
        );
        return;
    }

    let existing = match std::fs::read_to_string(&file_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", crate::messages::error::file_read_failed(&e));
            return;
        }
    };

    let section_header = crate::messages::markdown::PROGRESS_SECTION_HEADER;
    let new_section = format!("{section_header}\n\n{summary}\n");

    // Replace existing section, or append at the end.
    let updated = if let Some(start) = existing.find(section_header) {
        // Find the end: start of the next `## ` header, or EOF.
        let after_header = start + section_header.len();
        let end = existing[after_header..]
            .find("\n## ")
            .map(|pos| after_header + pos + 1)
            .unwrap_or(existing.len());

        format!(
            "{}{}\n{}",
            &existing[..start],
            new_section,
            &existing[end..]
        )
    } else {
        format!("{}\n{}\n", existing.trim_end(), new_section)
    };

    match std::fs::write(&file_path, updated) {
        Ok(()) => println!(
            "{}",
            crate::messages::status::markdown_saved(&file_path.display())
        ),
        Err(e) => eprintln!("{}", crate::messages::error::file_save_failed(&e)),
    }
}
