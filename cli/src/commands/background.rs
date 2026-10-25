//! Background process lifecycle, lock files, and notifications.

use super::{parse_date_flag, resolve_lang, run_today};
use crate::{config, output};

/// Returns the path to ~/.rwd/worker.lock
fn worker_lock_path() -> std::path::PathBuf {
    dirs::home_dir()
        .expect(crate::messages::error::HOME_DIR_NOT_FOUND)
        .join(".rwd")
        .join("worker.lock")
}

/// Returns the path to ~/.rwd/worker.log
pub(crate) fn worker_log_path() -> std::path::PathBuf {
    dirs::home_dir()
        .expect(crate::messages::error::HOME_DIR_NOT_FOUND)
        .join(".rwd")
        .join("worker.log")
}

/// Checks if a process with the given PID is alive (no unsafe, uses kill -0 command).
#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Checks if a lock file exists with a live process.
fn is_worker_running() -> bool {
    let lock_path = worker_lock_path();
    if !lock_path.exists() {
        return false;
    }
    if let Ok(contents) = std::fs::read_to_string(&lock_path)
        && let Ok(pid) = contents.trim().parse::<u32>()
        && is_process_alive(pid)
    {
        return true;
    }
    // Stale lock — remove it
    let _ = std::fs::remove_file(&lock_path);
    false
}

/// Spawns a background worker process.
pub(crate) fn spawn_worker(
    lang_flag: &Option<String>,
    date_flag: &Option<String>,
    no_cache: bool,
    include_automated: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if is_worker_running() {
        println!("{}", crate::messages::background::ALREADY_RUNNING);
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let mut args = vec!["today".to_string(), "--worker".to_string()];
    if let Some(lang) = lang_flag {
        args.push("--lang".to_string());
        args.push(lang.clone());
    } else {
        // Resolve lang from config now (before detaching) to avoid stdin prompt in worker.
        // Fail synchronously if language cannot be resolved.
        let mut loaded_config = config::load_config_if_exists();
        let lang = resolve_lang(&None, &mut loaded_config)?;
        args.push("--lang".to_string());
        args.push(lang.to_string());
    }
    if let Some(date) = date_flag {
        args.push("--date".to_string());
        args.push(date.clone());
    }
    if no_cache {
        args.push("--no-cache".to_string());
    }
    if include_automated {
        args.push("--include-automated".to_string());
    }

    let child = std::process::Command::new(exe)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    println!("  {}", crate::messages::background::starting(child.id()));
    println!("  {}", crate::messages::background::NOTIFIED_WHEN_DONE);

    // Show where results will be saved.
    let display_date =
        parse_date_flag(date_flag).unwrap_or_else(|_| chrono::Local::now().date_naive());
    if let Ok(vault_path) = output::load_vault_path() {
        let file_path = vault_path.join(format!("{display_date}.md"));
        println!(
            "  {}",
            crate::messages::background::results_path(&file_path.display())
        );
    }

    Ok(())
}

/// Runs as a background worker: lock, analyze, notify, unlock.
pub(crate) async fn run_worker(
    lang: Option<String>,
    target_date: chrono::NaiveDate,
    no_cache: bool,
    include_automated: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let lock_path = worker_lock_path();
    if let Some(parent) = lock_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&lock_path, std::process::id().to_string())?;

    let result = run_today(false, lang, target_date, no_cache, include_automated).await;

    // Always clean up lock file
    let _ = std::fs::remove_file(&lock_path);

    match result {
        Ok(()) => {
            // Clean up previous error log on success
            let log_path = worker_log_path();
            if log_path.exists() {
                let _ = std::fs::remove_file(&log_path);
            }
            send_notification(
                crate::messages::background::NOTIFY_TITLE,
                crate::messages::background::NOTIFY_SUCCESS,
            );
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Sends an OS notification via notify-rust.
pub(crate) fn send_notification(title: &str, body: &str) {
    let mut notification = notify_rust::Notification::new();
    notification.summary(title).body(body);

    #[cfg(target_os = "macos")]
    notification.sound_name(crate::messages::background::NOTIFY_SOUND);

    notification.show().ok();
}
