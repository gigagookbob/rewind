//! Collect, analyze, cache, and save a day of sessions.

use super::collection::{
    claude_include_automated, collect_claude_entries_with_stats, collect_codex_sessions,
};
use super::display::{
    GREEN, MAGENTA, RESET, YELLOW, format_roots_for_display, print_info_box, print_insights,
    print_logo_banner,
};
use super::resolve_lang;
use crate::{analyzer, cache, config, output, parser, redactor};

/// Parses session logs for the given date, runs LLM analysis, and prints insights.
pub(crate) async fn run_today(
    verbose: bool,
    lang_flag: Option<String>,
    target_date: chrono::NaiveDate,
    no_cache: bool,
    include_automated_flag: bool,
) -> Result<(), parser::ParseError> {
    let mut loaded_config = config::load_config_if_exists();
    if loaded_config.is_none() {
        eprintln!("{}", crate::messages::error::NO_CONFIG);
        std::process::exit(1);
    }
    let redactor_enabled = loaded_config.as_ref().unwrap().is_redactor_enabled();
    let lang = resolve_lang(&lang_flag, &mut loaded_config)
        .map_err(|e| -> Box<dyn std::error::Error> { e })?;

    let today = target_date;
    let include_automated =
        include_automated_flag || claude_include_automated(loaded_config.as_ref());

    // === Collect Claude Code logs ===
    let (claude_entries, claude_discovery) =
        collect_claude_entries_with_stats(today, loaded_config.as_ref(), include_automated);

    // === Collect Codex logs ===
    let (codex_sessions, codex_roots) = collect_codex_sessions(today, loaded_config.as_ref());

    let codex_entry_count: usize = codex_sessions
        .iter()
        .map(|(_, entries)| entries.len())
        .sum();

    if verbose {
        eprintln!(
            "{}",
            crate::messages::verbose::discover_stats(
                claude_discovery.project_count,
                claude_discovery.file_count,
                claude_discovery.total_entries,
                claude_entries.len() + codex_entry_count,
            )
        );
        eprintln!(
            "{}",
            crate::messages::verbose::claude_session_mix(
                claude_discovery.claude_interactive_sessions,
                claude_discovery.claude_automated_sessions,
                include_automated,
            )
        );
        if !claude_discovery.roots.is_empty() {
            eprintln!(
                "{}",
                crate::messages::verbose::used_roots(
                    "Claude",
                    &format_roots_for_display(&claude_discovery.roots)
                )
            );
        }
        if !codex_roots.is_empty() {
            eprintln!(
                "{}",
                crate::messages::verbose::used_roots(
                    "Codex",
                    &format_roots_for_display(&codex_roots)
                )
            );
        }
    }

    if claude_entries.is_empty() && codex_sessions.is_empty() {
        println!("No log entries found for today ({today}).");
        return Ok(());
    }

    let claude_count = claude_entries.len();
    let codex_session_count = codex_sessions.len();

    // === Logo banner ===
    print_logo_banner();

    // === Info box ===
    let summaries = if !claude_entries.is_empty() {
        Some(parser::summarize_entries(&claude_entries))
    } else {
        None
    };

    print_info_box(
        today,
        summaries.as_deref(),
        &claude_entries,
        &codex_sessions,
    );

    // === Cache check ===
    // Reuse previous analysis if the entry count is unchanged.
    if no_cache {
        println!("\n{}", crate::messages::status::CACHE_BYPASSED);
    } else if let Some(cached) = cache::load_cache(today) {
        let count_matches = cached.claude_entry_count == claude_count
            && cached.codex_session_count == codex_session_count
            && cached.codex_entry_count == codex_entry_count;
        let timezone_matches = cache::is_timezone_compatible(&cached, today);

        if count_matches && timezone_matches {
            println!("\n{}", crate::messages::status::CACHE_USED);
            let source_refs: Vec<(&str, &analyzer::AnalysisResult)> = cached
                .sources
                .iter()
                .map(|(name, result)| (name.as_str(), result))
                .collect();
            for (name, analysis) in &source_refs {
                print_insights(name, analysis);
            }
            save_combined_analysis(&source_refs, today, verbose);
            return Ok(());
        }

        if count_matches && !timezone_matches {
            eprintln!(
                "{YELLOW}{}{RESET}",
                crate::messages::status::CACHE_STALE_TIMEZONE
            );
            eprintln!("{}", crate::messages::status::CACHE_STALE_HINT);
            eprintln!();
        }
    }

    // === LLM analysis ===
    let provider_label = analyzer::provider::load_provider()
        .map(|(p, _)| p.display_name().to_string())
        .unwrap_or_else(|_| "LLM".to_string());
    println!(
        "\n{MAGENTA}{}{RESET}",
        crate::messages::status::analyzing(&provider_label)
    );

    let analysis_context = analyzer::prepare_session_analysis_context().await?;
    let mut sources: Vec<(String, analyzer::AnalysisResult)> = Vec::new();
    let mut total_redact = redactor::RedactResult::empty();

    // Claude analysis
    if !claude_entries.is_empty() {
        let (result, redact_result) = analyzer::analyze_entries(
            &claude_entries,
            redactor_enabled,
            verbose,
            &lang,
            "Claude Code",
            &analysis_context,
        )
        .await?;
        total_redact.merge(redact_result);
        sources.push(("Claude Code".to_string(), result));
    }

    // Codex analysis — unified session pipeline (same executor as Claude).
    if !codex_sessions.is_empty() {
        let mut codex_tasks = Vec::new();
        for (summary, entries) in &codex_sessions {
            let task = analyzer::build_codex_session_task(entries, &summary.session_id)?;
            codex_tasks.push(task);
        }
        let (result, redact_result) = analyzer::analyze_session_tasks(
            codex_tasks,
            redactor_enabled,
            verbose,
            &lang,
            "Codex",
            &analysis_context,
        )
        .await?;
        total_redact.merge(redact_result);
        sources.push(("Codex".to_string(), result));
    }

    // Output and save results
    if total_redact.total_count > 0 {
        println!(
            "{}",
            crate::messages::status::redacted(
                total_redact.total_count,
                &total_redact.format_summary()
            )
        );
    }

    if !sources.is_empty() {
        let source_refs: Vec<(&str, &analyzer::AnalysisResult)> = sources
            .iter()
            .map(|(name, result)| (name.as_str(), result))
            .collect();
        for (name, analysis) in &source_refs {
            print_insights(name, analysis);
        }
        save_combined_analysis(&source_refs, today, verbose);

        // Save analysis results to cache.
        let cache_data = cache::TodayCache {
            date: today.to_string(),
            claude_entry_count: claude_count,
            codex_session_count,
            codex_entry_count,
            timezone_window_key: cache::timezone_window_key(today),
            sources,
        };
        if let Err(e) = cache::save_cache(&cache_data, today) {
            eprintln!("{}", crate::messages::error::cache_save_failed(&e));
        } else if verbose {
            let cache_path = dirs::home_dir()
                .expect(crate::messages::error::HOME_DIR_NOT_FOUND)
                .join(".rwd")
                .join("cache")
                .join(format!("today-{today}.json"));
            if let Ok(meta) = std::fs::metadata(&cache_path) {
                eprintln!(
                    "{}",
                    crate::messages::verbose::cache_saved(
                        &cache_path.display(),
                        meta.len() as f64 / 1024.0,
                    )
                );
            }
        }

        println!("\n{GREEN}{}{RESET}", crate::messages::status::REWIND_DONE);
    }

    Ok(())
}

/// Combines analysis results from multiple sources and saves as Markdown.
fn save_combined_analysis(
    sources: &[(&str, &analyzer::AnalysisResult)],
    date: chrono::NaiveDate,
    verbose: bool,
) {
    let vault_path = match output::load_vault_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}", crate::messages::error::vault_path_load_failed(&e));
            return;
        }
    };

    let markdown = output::render_combined_markdown(sources, date);

    match output::save_to_vault(&vault_path, date, &markdown) {
        Ok(saved) => {
            println!(
                "\n{}",
                crate::messages::status::markdown_saved(&saved.display())
            );
            if verbose && let Ok(meta) = std::fs::metadata(&saved) {
                eprintln!(
                    "{}",
                    crate::messages::verbose::markdown_file_size(
                        &saved.display(),
                        meta.len() as f64 / 1024.0,
                    )
                );
            }
        }
        Err(e) => eprintln!("{}", crate::messages::error::file_save_failed(&e)),
    }
}
