//! Preview and reset local configuration, cache, and worker state.

use super::{ConfigError, config_path};
use dialoguer::Confirm;
use dialoguer::theme::ColorfulTheme;
use std::path::PathBuf;

fn reset_targets() -> Result<Vec<PathBuf>, ConfigError> {
    let home = dirs::home_dir().ok_or(crate::messages::error::HOME_DIR_NOT_FOUND)?;
    Ok(vec![
        config_path()?,
        home.join(".rwd").join("cache"),
        home.join(".rwd").join("worker.lock"),
        home.join(".rwd").join("worker.log"),
    ])
}

/// `rwd reset` — removes local config/cache state so users can recover from bad state.
pub(crate) fn run_reset(yes: bool, dry_run: bool) -> Result<(), ConfigError> {
    let targets = reset_targets()?;
    let existing: Vec<PathBuf> = targets.into_iter().filter(|p| p.exists()).collect();

    if existing.is_empty() {
        println!("{}", crate::messages::reset::NOTHING_TO_RESET);
        return Ok(());
    }

    if dry_run {
        println!("{}", crate::messages::reset::DRY_RUN_HEADER);
        for path in &existing {
            println!("{}", crate::messages::reset::item(&path.display()));
        }
        return Ok(());
    }

    if !yes {
        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(crate::messages::reset::CONFIRM)
            .default(false)
            .interact_opt()?;
        if confirmed != Some(true) {
            println!("{}", crate::messages::reset::CANCELLED);
            return Ok(());
        }
    }

    for path in &existing {
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
    }

    println!("{}", crate::messages::reset::REMOVED_HEADER);
    for path in &existing {
        println!("{}", crate::messages::reset::item(&path.display()));
    }
    println!("{}", crate::messages::reset::NEXT_STEP);
    Ok(())
}
