mod analyzer;
mod cache;
mod cli;
mod commands;
mod config;
mod doctor;
mod messages;
mod output;
mod parser;
mod redactor;
mod update;

#[cfg(target_os = "windows")]
compile_error!(
    "Native Windows is not supported. Please install WSL2 and run rwd in a Linux environment."
);

use clap::Parser;
use cli::{AuthAction, Commands};

use commands::{
    parse_date_flag, run_slack, run_summary, run_today, run_worker, send_notification,
    spawn_worker, worker_log_path,
};

#[tokio::main]
async fn main() {
    let args = cli::Cli::parse();

    // Show update notification only for synchronous commands.
    // Worker mode skips this to avoid blocking (no terminal).
    let skip_update = matches!(
        args.command,
        Commands::Update | Commands::Doctor | Commands::Reset { .. } | Commands::Version
    ) || matches!(args.command, Commands::Today { worker: true, .. });
    if !skip_update {
        update::notify_if_update_available().await;
    }

    match args.command {
        Commands::Today {
            verbose,
            lang,
            date,
            background,
            no_cache,
            include_automated,
            worker,
        } => {
            let target_date = match parse_date_flag(&date) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            if worker {
                if let Err(e) = run_worker(lang, target_date, no_cache, include_automated).await {
                    let log_path = worker_log_path();
                    if let Some(parent) = log_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let error_text = format!("{e}");
                    let (redacted_error, _) = redactor::redact_text(&error_text);
                    let _ = std::fs::write(&log_path, redacted_error);
                    send_notification(
                        crate::messages::background::NOTIFY_TITLE,
                        &crate::messages::background::notify_failure(&log_path.display()),
                    );
                    std::process::exit(1);
                }
            } else if background {
                if let Err(e) = spawn_worker(&lang, &date, no_cache, include_automated) {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            } else {
                if let Err(e) =
                    run_today(verbose, lang, target_date, no_cache, include_automated).await
                {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Init => {
            if let Err(e) = config::run_init() {
                eprintln!("{}", crate::messages::error::init_failed(&e));
                std::process::exit(1);
            }
        }
        Commands::Config { key, value } => {
            let result = match (key, value) {
                (Some(k), Some(v)) => config::run_config(&k, &v),
                (None, None) => config::run_config_interactive().await,
                _ => Err(crate::messages::config::USAGE.into()),
            };
            if let Err(e) = result {
                eprintln!("{}", crate::messages::error::config_failed(&e));
                std::process::exit(1);
            }
        }
        Commands::Update => {
            if let Err(e) = update::run_update().await {
                eprintln!("{}", crate::messages::error::update_failed(&e));
                std::process::exit(1);
            }
        }
        Commands::Reset { yes, dry_run } => {
            if let Err(e) = config::run_reset(yes, dry_run) {
                eprintln!("{}", crate::messages::error::config_failed(&e));
                std::process::exit(1);
            }
        }
        Commands::Doctor => {
            if let Err(e) = doctor::run_doctor() {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Version => {
            println!("rwd {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Auth { action } => match action {
            AuthAction::Status => {
                if let Err(e) = config::run_auth_status().await {
                    eprintln!("{}", crate::messages::error::config_failed(&e));
                    std::process::exit(1);
                }
            }
        },
        Commands::Summary {
            lang,
            date,
            no_cache,
        } => {
            let target_date = match parse_date_flag(&date) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            if let Err(e) = run_summary(lang, target_date, no_cache).await {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Slack {
            lang,
            date,
            no_cache,
        } => {
            let target_date = match parse_date_flag(&date) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            if let Err(e) = run_slack(lang, target_date, no_cache).await {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}
