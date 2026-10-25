//! Collect, analyze, cache, and save a day of sessions.

use super::collection::collect_sources;
use super::display::{
    GREEN, MAGENTA, RESET, YELLOW, format_roots_for_display, print_info_box, print_insights,
    print_logo_banner,
};
use super::resolve_lang;
use crate::{analyzer, cache, config, output, parser, redactor, sources};

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

    // === Collect logs from every registered source ===
    let adapters = sources::registered_adapters();
    let run = collect_sources(
        &adapters,
        &sources::CollectContext {
            date: today,
            config: loaded_config.as_ref(),
            include_automated: include_automated_flag,
        },
    );

    for (source_name, error) in &run.failures {
        eprintln!(
            "{YELLOW}{}{RESET}",
            crate::messages::status::source_collect_failed(source_name, error)
        );
    }

    if verbose {
        for source in &run.sources {
            for line in &source.collection.verbose_lines {
                eprintln!("{line}");
            }
            for (reason, count) in &source.collection.excluded {
                if *count > 0 {
                    eprintln!(
                        "{}",
                        crate::messages::verbose::excluded_sessions(
                            source.display_name,
                            reason,
                            *count
                        )
                    );
                }
            }
            if !source.collection.roots.is_empty() {
                eprintln!(
                    "{}",
                    crate::messages::verbose::used_roots(
                        source.display_name,
                        &format_roots_for_display(&source.collection.roots)
                    )
                );
            }
        }
    }

    if run.session_count() == 0 {
        println!("No log entries found for today ({today}).");
        return Ok(());
    }

    // === Logo banner ===
    print_logo_banner();

    // === Info box ===
    print_info_box(today, &run.sources);

    // === Cache check ===
    // Reuse previous analysis if every source fingerprint is unchanged.
    let fingerprints = run.fingerprint_table();
    if no_cache {
        println!("\n{}", crate::messages::status::CACHE_BYPASSED);
    } else if let Some(cached) = cache::load_cache(today) {
        let count_matches = cache::fingerprints_match(&cached, &fingerprints);
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
    let mut results: Vec<(String, analyzer::AnalysisResult)> = Vec::new();
    let mut total_redact = redactor::RedactResult::empty();

    // One shared session pipeline per source, in adapter order.
    for source in run.sources {
        if source.collection.sessions.is_empty() {
            continue;
        }
        let tasks = source
            .collection
            .sessions
            .into_iter()
            .map(|session| session.task)
            .collect();
        let (result, redact_result) = analyzer::analyze_session_tasks(
            tasks,
            redactor_enabled,
            verbose,
            &lang,
            source.display_name,
            &analysis_context,
        )
        .await?;
        total_redact.merge(redact_result);
        results.push((source.display_name.to_string(), result));
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

    if !results.is_empty() {
        let source_refs: Vec<(&str, &analyzer::AnalysisResult)> = results
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
            source_fingerprints: fingerprints,
            timezone_window_key: cache::timezone_window_key(today),
            sources: results,
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
