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
        return Ok(prepare_cache_for_summary(cached, &run, config.as_ref()));
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
    Ok(prepare_cache_for_summary(cached, &run, config.as_ref()))
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

/// Use the same local project and source checks as the today cache display.
fn prepare_cache_for_summary(
    mut cached: cache::TodayCache,
    run: &super::collection::CollectionRun,
    config: Option<&config::Config>,
) -> cache::TodayCache {
    cached.sources = super::today::prepare_cached_sources(
        &cached,
        run,
        config.is_none_or(|c| c.is_redactor_enabled()),
    );
    cached
}

/// Opaque per-run project keys avoid sending directories or merging equal names.
fn collect_work_summaries(cached: &cache::TodayCache) -> String {
    use crate::analyzer::retrospective::Project;
    use std::collections::{HashMap, HashSet};
    let project = |session: &analyzer::insight::SessionInsight| {
        session
            .project
            .as_ref()
            .and_then(|p| Project::from_directories([p.directory.as_str()]))
    };
    let mut names: HashMap<String, HashSet<Project>> = HashMap::new();
    for (_, analysis) in &cached.sources {
        for session in &analysis.sessions {
            if let Some(p) = project(session) {
                names.entry(p.name().into()).or_default().insert(p);
            }
        }
    }
    let mut keys = HashMap::new();
    let mut next_key = 1;
    let mut records = Vec::new();
    for (source_name, analysis) in &cached.sources {
        for session in &analysis.sessions {
            let (key, label) = if let Some(p) = project(session) {
                let key = *keys.entry(p.clone()).or_insert_with(|| {
                    let key = next_key;
                    next_key += 1;
                    key
                });
                let label = if names.get(p.name()).is_some_and(|set| set.len() > 1) {
                    format!("{} (project {key})", p.name())
                } else {
                    p.name().to_string()
                };
                (key, label)
            } else {
                let key = next_key;
                next_key += 1;
                (
                    key,
                    format!("Project unknown / 프로젝트 미확인 (session {key})"),
                )
            };
            records.push(serde_json::json!({
                "project_key": format!("p{key}"), "project_label":label,
                "source":source_name, "session_id":session.session_id,
                "task_title":session.task_title.as_ref().map(|t| &t.text),
                "work_summary":session.work_summary,
            }));
        }
    }
    if records.is_empty() {
        String::new()
    } else {
        serde_json::Value::Array(records).to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_work_summaries_keep_project_identity_without_transmitting_paths() {
        let session = |id: &str, path: Option<&str>| {
            serde_json::json!({
                "session_id":id,"work_summary":"work", "decisions":[],"curiosities":[],"corrections":[],
                "project":path.map(|p| serde_json::json!({"directory":p}))
            })
        };
        let cached: cache::TodayCache = serde_json::from_value(serde_json::json!({
            "date":"2099-01-01", "sources":[
                ["Claude Code", {"sessions":[session("a",Some("/private/a/app")),session("b",Some("/private/b/app")),session("u",None)]}],
                ["Codex", {"sessions":[session("c",Some("/private/a/app")),session("v",None)]}]
            ]
        })).unwrap();
        let text = collect_work_summaries(&cached);
        assert!(!text.contains("/private/"));
        let rows: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(rows[0]["project_key"], rows[3]["project_key"]);
        assert_ne!(rows[0]["project_key"], rows[1]["project_key"]);
        assert_ne!(rows[0]["project_label"], rows[1]["project_label"]);
        assert_ne!(rows[2]["project_key"], rows[4]["project_key"]);
        assert_eq!(rows[3]["source"], "Codex");
        assert_eq!(rows[3]["session_id"], "c");
        assert!(rows[0].get("questions").is_none());
        assert!(rows[0].get("follow_ups").is_none());
    }
}

#[cfg(test)]
mod cache_project_tests {
    use super::*;

    #[test]
    fn test_summary_uses_current_local_project_instead_of_cached_or_ai_identity() {
        let cached: cache::TodayCache = serde_json::from_value(serde_json::json!({
            "date":"2099-01-01", "sources":[["Codex",{"sessions":[{
                "session_id":"s1","work_summary":"work","decisions":[],"curiosities":[],"corrections":[],
                "project":{"directory":"/impostor/app"}, "task_title":{"text":"unverified","evidence":[]}
            }]}]]
        })).unwrap();
        let run = super::super::collection::CollectionRun {
            failures: vec![],
            sources: vec![super::super::collection::CollectedSource {
                id: "codex",
                display_name: "Codex",
                collection: sources::SourceCollection {
                    sessions: vec![sources::CollectedSession {
                        task: analyzer::SessionTask {
                            project: analyzer::retrospective::Project::from_directories([
                                "/actual/app",
                            ]),
                            source: "codex",
                            session_id: "s1".into(),
                            prompt_text: String::new(),
                            messages: vec![],
                            evidence_sources: vec![],
                            estimated_tokens: 0,
                        },
                        first_activity: None,
                        last_activity: None,
                        token_usage: None,
                    }],
                    ..sources::SourceCollection::default()
                },
            }],
        };
        let prepared = prepare_cache_for_summary(cached, &run, None);
        assert_eq!(
            prepared.sources[0].1.sessions[0]
                .project
                .as_ref()
                .unwrap()
                .directory,
            "/actual/app"
        );
        assert!(prepared.sources[0].1.sessions[0].task_title.is_none());
        let text = collect_work_summaries(&prepared);
        assert!(
            !text.contains("/actual/")
                && !text.contains("/impostor/")
                && !text.contains("unverified")
        );
    }
}
