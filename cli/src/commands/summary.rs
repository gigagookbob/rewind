//! Summary and Slack text workflows built from cached daily analyses.

use super::collection::collect_sources;
use super::display::{RESET, YELLOW};
use super::{resolve_lang, run_today};
use crate::{analyzer, cache, config, output, sources};

/// Loads today's analysis cache, running the full analysis first when the
/// cache is missing or stale for the current timezone rules.
async fn load_cache_or_analyze(
    today: chrono::NaiveDate,
) -> Result<cache::TodayCache, Box<dyn std::error::Error>> {
    let config = config::load_config_if_exists();
    let run = collect_sources(
        &sources::registered_adapters(),
        &sources::CollectContext {
            date: today,
            config: config.as_ref(),
            include_automated: false,
        },
    );
    if !run.failures.is_empty() {
        return Err("Cannot verify cached analysis against current session logs".into());
    }
    let fingerprints = run.fingerprint_table();
    let cached = cache::load_cache(today);
    if cached.is_none() {
        println!("{}", crate::messages::error::NO_CACHE);
    }
    if let Some(cached) = cached
        && cache::is_timezone_compatible(&cached, today)
        && cache::fingerprints_match(&cached, &fingerprints)
    {
        return Ok(cached);
    }

    eprintln!(
        "{YELLOW}{}{RESET}",
        crate::messages::status::CACHE_STALE_HINT
    );
    eprintln!();
    run_today(false, None, today, false, false).await?;
    let cached = reload_cache_after_analysis(today);
    if !cache::is_timezone_compatible(&cached, today)
        || !cache::fingerprints_match(&cached, &fingerprints)
    {
        return Err("No current daily analysis is available after refreshing the cache".into());
    }
    Ok(cached)
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
    append_summary_to_markdown(today, &summary)?;

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

    // Warn if cache is stale (source fingerprint mismatch).
    let mut loaded_config = config::load_config_if_exists();
    let adapters = sources::registered_adapters();
    let run = collect_sources(
        &adapters,
        &sources::CollectContext {
            date: today,
            config: loaded_config.as_ref(),
            include_automated: false,
        },
    );
    let current = run.fingerprint_table();
    if !cache::fingerprints_match(&cached, &current) {
        let cached_total = cache::total_entry_count(&cached.source_fingerprints);
        let current_total = cache::total_entry_count(&current);
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

/// Updates only the unchanged rwd summary region, preserving user headings.
fn append_summary_to_markdown(
    date: chrono::NaiveDate,
    summary: &str,
) -> Result<(), output::OutputError> {
    let vault_path = output::load_vault_path()
        .map_err(|e| crate::messages::error::vault_path_load_failed(&e))?;
    let saved = output::save_summary_to_vault(&vault_path, date, summary)
        .map_err(|e| crate::messages::error::file_save_failed(&e))?;
    println!(
        "{}",
        crate::messages::status::markdown_saved(&saved.display())
    );
    Ok(())
}
