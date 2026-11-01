//! Collect, analyze, cache, and save a day of sessions.

use super::collection::{CollectionRun, collect_sources};
use super::display::{
    GREEN, MAGENTA, RESET, YELLOW, format_roots_for_display, print_dry_run, print_info_box,
    print_insights, print_logo_banner,
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

    print_collection_notes(&run, verbose);

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
            let cached_sources = prepare_cached_sources(cached, &run, redactor_enabled);
            let source_refs: Vec<(&str, &analyzer::AnalysisResult)> = cached_sources
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

fn prepare_cached_sources(
    cached: cache::TodayCache,
    run: &CollectionRun,
    redactor_enabled: bool,
) -> Vec<(String, analyzer::AnalysisResult)> {
    let mut results = cached.sources;
    for (name, analysis) in &mut results {
        let current = run
            .sources
            .iter()
            .find(|source| source.display_name == name);
        for session in &mut analysis.sessions {
            let sources = current
                .and_then(|source| {
                    source
                        .collection
                        .sessions
                        .iter()
                        .find(|candidate| candidate.task.session_id == session.session_id)
                })
                .map(|candidate| candidate.task.evidence_sources.as_slice())
                .unwrap_or_default();
            analyzer::evidence::revalidate_cached_session(session, sources, redactor_enabled);
        }
    }
    results
}

/// `rwd today --dry-run`: collects sessions and lists them without touching
/// the LLM provider, cache, or Markdown output. Works without a config file
/// (adapters then look only at their default log roots).
pub(crate) fn run_today_dry_run(
    verbose: bool,
    target_date: chrono::NaiveDate,
    include_automated_flag: bool,
) {
    let loaded_config = config::load_config_if_exists();
    let adapters = sources::registered_adapters();
    dry_run(
        &adapters,
        &sources::CollectContext {
            date: target_date,
            config: loaded_config.as_ref(),
            include_automated: include_automated_flag,
        },
        verbose,
    );
}

/// Collects from `adapters` and prints the dry-run report. Takes only the
/// adapter list and collection inputs, so it cannot reach the LLM, the cache,
/// or the vault.
fn dry_run(
    adapters: &[Box<dyn sources::SourceAdapter>],
    ctx: &sources::CollectContext,
    verbose: bool,
) -> CollectionRun {
    let run = collect_sources(adapters, ctx);
    print_collection_notes(&run, verbose);
    print_dry_run(ctx.date, &run.sources);
    run
}

/// Prints adapter failures, and with `-v` each source's diagnostics.
fn print_collection_notes(run: &CollectionRun, verbose: bool) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::claude::ClaudeAdapter;
    use crate::sources::codex::CodexAdapter;

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "{prefix}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ))
    }

    fn list_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).expect("read dir").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path.clone());
                }
                found.push(path);
            }
        }
        found.sort();
        found
    }

    #[test]
    fn test_cached_sources_downgrade_shifted_lines_without_reanalysis() {
        let base = unique_temp_dir("rwd_test_cached_location");
        let path = base.join("project/session.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let log = concat!(
            r#"{"type":"user","sessionId":"s1","timestamp":"2099-01-01T12:00:00Z","uuid":"u1","message":{"content":"A function is enough."}}"#,
            "\n"
        );
        std::fs::write(&path, log).unwrap();
        let adapters: Vec<Box<dyn sources::SourceAdapter>> =
            vec![Box::new(ClaudeAdapter::with_roots(vec![base.clone()]))];
        let date = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        let ctx = sources::CollectContext {
            date,
            config: None,
            include_automated: false,
        };
        let mut original = collect_sources(&adapters, &ctx);
        let task = &original.sources[0].collection.sessions[0].task;
        let (_, catalog) =
            analyzer::evidence::annotate_messages(&task.messages, &task.evidence_sources);
        let mut analysis = analyzer::insight::parse_response(r#"{"sessions":[{
            "session_id":"s1","work_summary":"synthetic","curiosities":[],"corrections":[],
            "decisions":[{"what":"Use a function","why":"","evidence":[{"message_id":"m1","quote":"A function is enough."}]}],
            "til":[{"title":"synthetic learning","detail":"","evidence":[{"message_id":"m1","quote":"A function is enough."}]}]
        }]}"#).unwrap();
        analyzer::evidence::validate(&mut analysis, "s1", &catalog);
        assert_eq!(
            analysis.sessions[0].decisions[0].evidence[0]
                .location
                .as_ref()
                .unwrap()
                .line,
            1
        );
        let cached = cache::TodayCache {
            date: date.to_string(),
            source_fingerprints: original.fingerprint_table(),
            timezone_window_key: cache::timezone_window_key(date),
            sources: vec![("Claude Code".into(), analysis)],
        };
        let saved_cache = serde_json::to_string(&cached).unwrap();
        let unchanged =
            prepare_cached_sources(serde_json::from_str(&saved_cache).unwrap(), &original, true);
        assert!(
            unchanged[0].1.sessions[0].decisions[0].evidence[0]
                .location
                .is_some()
        );
        for missing in ["source", "session"] {
            if missing == "source" {
                original.sources[0].display_name = "Codex";
            } else {
                original.sources[0].collection.sessions[0].task.session_id = "other".into();
            }
            let prepared = prepare_cached_sources(
                serde_json::from_str(&saved_cache).unwrap(),
                &original,
                false,
            );
            assert!(
                prepared[0].1.sessions[0].decisions[0].evidence[0]
                    .location
                    .is_none(),
                "{missing}"
            );
            original.sources[0].display_name = "Claude Code";
            original.sources[0].collection.sessions[0].task.session_id = "s1".into();
        }
        // A physical line change now invalidates the versioned input digest.
        std::fs::write(&path, format!("\n{log}")).unwrap();
        let current = collect_sources(&adapters, &ctx);
        std::fs::remove_dir_all(&base).unwrap();
        assert!(!cache::fingerprints_match(
            &cached,
            &current.fingerprint_table()
        ));
        assert!(cache::is_timezone_compatible(&cached, date));
        let prepared = prepare_cached_sources(cached, &current, true);
        let session = &prepared[0].1.sessions[0];
        assert_eq!(session.decisions[0].what, "Use a function");
        assert!(session.decisions[0].evidence[0].location.is_none());
        assert!(session.til[0].evidence[0].location.is_none());
        let md = output::render_combined_markdown(&[("Claude Code", &prepared[0].1)], date);
        assert!(md.contains("근거 미확인"));
        assert!(!md.contains(&format!("{}:1", path.display())));
    }

    #[test]
    fn test_dry_run_collects_without_config_and_writes_nothing() {
        let base = unique_temp_dir("rwd_test_today_dry_run");
        let claude_root = base.join("claude");
        let codex_root = base.join("codex");
        std::fs::create_dir_all(claude_root.join("project-a")).expect("claude project");
        std::fs::create_dir_all(&codex_root).expect("codex root");
        std::fs::write(
            claude_root.join("project-a/session.jsonl"),
            concat!(
                r#"{"type":"user","sessionId":"claude-s1","timestamp":"2099-01-01T12:00:00Z","uuid":"u1","message":{"role":"user","content":"hello"}}"#,
                "\n",
            ),
        )
        .expect("write log");
        let before = list_files(&base);

        let adapters: Vec<Box<dyn sources::SourceAdapter>> = vec![
            Box::new(ClaudeAdapter::with_roots(vec![claude_root])),
            Box::new(CodexAdapter::with_roots(vec![codex_root])),
        ];
        let ctx = sources::CollectContext {
            date: chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date"),
            config: None,
            include_automated: false,
        };
        let run = dry_run(&adapters, &ctx, true);
        let after = list_files(&base);
        std::fs::remove_dir_all(&base).ok();

        assert!(run.failures.is_empty());
        assert_eq!(run.session_count(), 1);
        assert_eq!(before, after);
    }
}
