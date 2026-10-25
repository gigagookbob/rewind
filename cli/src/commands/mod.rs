//! Command workflows and their collection, display, and background support.

mod background;
mod collection;
mod display;
mod summary;
mod today;

use crate::config;

pub(crate) use background::{run_worker, send_notification, spawn_worker, worker_log_path};
pub(crate) use summary::{run_slack, run_summary};
pub(crate) use today::{run_today, run_today_dry_run};

/// Parses `--date` flag into `NaiveDate`. Returns today if `None`.
pub(crate) fn parse_date_flag(date: &Option<String>) -> Result<chrono::NaiveDate, String> {
    match date {
        None => Ok(chrono::Local::now().date_naive()),
        Some(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map_err(|_| format!("Invalid date format: '{s}'. Expected YYYY-MM-DD.")),
    }
}

/// Resolves the language from: --lang flag > config > migration prompt.
fn resolve_lang(
    flag: &Option<String>,
    loaded_config: &mut Option<config::Config>,
) -> Result<config::Lang, Box<dyn std::error::Error>> {
    // 1. --lang flag takes priority
    if let Some(lang_str) = flag {
        return match lang_str.as_str() {
            "ko" => Ok(config::Lang::Ko),
            "en" => Ok(config::Lang::En),
            _ => Err(crate::messages::lang::unsupported(lang_str).into()),
        };
    }
    // 2. Config value
    if let Some(cfg) = loaded_config.as_ref()
        && let Some(lang) = &cfg.lang
    {
        return Ok(lang.clone());
    }
    // 3. Migration prompt — ask user and save to config
    eprint!("{}", crate::messages::lang::NOT_CONFIGURED);
    use std::io::Write;
    let _ = std::io::stderr().flush();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let lang = match input.trim() {
        "ko" => config::Lang::Ko,
        _ => config::Lang::En,
    };
    // Save to config if available
    if let Some(cfg) = loaded_config.as_mut() {
        cfg.lang = Some(lang.clone());
        if let Ok(config_file) = config::config_path() {
            let _ = config::save_config(cfg, &config_file);
        }
        eprintln!("{}", crate::messages::lang::saved(&lang.to_string()));
    }
    Ok(lang)
}
