//! Provider authentication status, verification, and masked key display.

use super::{ConfigError, LlmConfig, config_path, load_config, resolve_env_placeholders};

pub(super) fn provider_auth_method(provider: &str) -> &'static str {
    match provider {
        "codex" => "Codex login session",
        "anthropic" | "openai" => "API key",
        _ => "API key",
    }
}

pub(super) fn provider_uses_api_key(provider: &str) -> bool {
    matches!(provider, "anthropic" | "openai")
}

fn has_stored_api_key(api_key: &str) -> bool {
    !api_key.trim().is_empty()
}

pub(super) fn print_provider_switch_guidance(
    old_provider: &str,
    new_provider: &str,
    llm: &LlmConfig,
) {
    if old_provider == new_provider {
        return;
    }

    if new_provider == "codex" {
        let openai_state = if has_stored_api_key(&llm.openai_api_key) {
            "set"
        } else {
            "not set"
        };
        let anthropic_state = if has_stored_api_key(&llm.anthropic_api_key) {
            "set"
        } else {
            "not set"
        };
        eprintln!(
            "{}",
            crate::messages::config::switched_to_codex_keeps_api_key(&format!(
                "openai: {openai_state}, anthropic: {anthropic_state}"
            ))
        );
    } else if provider_uses_api_key(new_provider)
        && !has_stored_api_key(llm.api_key_for_provider(new_provider))
    {
        eprintln!(
            "{}",
            crate::messages::config::provider_requires_api_key(new_provider)
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexLoginState {
    Verified,
    NotLoggedIn,
    CheckFailed,
}

fn infer_codex_login_state(success: bool, stdout: &str, stderr: &str) -> CodexLoginState {
    if !success {
        return CodexLoginState::NotLoggedIn;
    }

    // Some Codex builds print login status to stderr instead of stdout.
    let merged = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    if merged.contains("not logged in") || merged.contains("not authenticated") {
        CodexLoginState::NotLoggedIn
    } else if merged.contains("logged in") {
        CodexLoginState::Verified
    } else {
        CodexLoginState::NotLoggedIn
    }
}

async fn codex_login_state() -> CodexLoginState {
    let status_output = tokio::task::spawn_blocking(|| {
        std::process::Command::new("codex")
            .args(["login", "status"])
            .output()
    })
    .await;

    match status_output {
        Ok(Ok(output)) => infer_codex_login_state(
            output.status.success(),
            &String::from_utf8_lossy(&output.stdout),
            &String::from_utf8_lossy(&output.stderr),
        ),
        _ => CodexLoginState::CheckFailed,
    }
}

pub(crate) async fn run_auth_status() -> Result<(), ConfigError> {
    let config_file = config_path()?;

    if !config_file.exists() {
        return Err(crate::messages::config::NO_CONFIG.into());
    }

    let config = load_config(&config_file)?;
    let provider = config.llm.provider.as_str();
    let openai_has_key = has_stored_api_key(&config.llm.openai_api_key);
    let anthropic_has_key = has_stored_api_key(&config.llm.anthropic_api_key);

    println!("{}", crate::messages::auth::provider(provider));
    println!(
        "{}",
        crate::messages::auth::auth_method(provider_auth_method(provider))
    );

    let openai_detail = if provider == "openai" {
        "active"
    } else {
        "inactive"
    };
    let anthropic_detail = if provider == "anthropic" {
        "active"
    } else {
        "inactive"
    };
    println!(
        "{}",
        crate::messages::auth::provider_api_key(
            "OpenAI",
            if openai_has_key { "set" } else { "not set" },
            openai_detail,
        )
    );
    println!(
        "{}",
        crate::messages::auth::provider_api_key(
            "Anthropic",
            if anthropic_has_key { "set" } else { "not set" },
            anthropic_detail,
        )
    );

    if provider == "codex" {
        let state = match codex_login_state().await {
            CodexLoginState::Verified => "verified",
            CodexLoginState::NotLoggedIn => "not logged in",
            CodexLoginState::CheckFailed => "check failed",
        };
        println!("{}", crate::messages::auth::codex_login_status(state));
    } else {
        let has_active_key = has_stored_api_key(config.llm.api_key_for_provider(provider));
        if !has_active_key {
            let hint = if provider == "openai" {
                "set with `rwd config api-key <key>` or `rwd config openai-api-key <key>`"
            } else {
                "set with `rwd config api-key <key>` or `rwd config anthropic-api-key <key>`"
            };
            println!(
                "{}",
                crate::messages::auth::provider_missing_api_key(provider, hint)
            );
        }
    }

    Ok(())
}

/// Verifies provider credentials.
/// - anthropic/openai: lightweight models endpoint call
/// - codex: `codex login status` command check
pub(super) async fn verify_api_key(provider: &str, api_key: &str) {
    let dim = "\x1b[2m";
    let green = "\x1b[32m";
    let yellow = "\x1b[33m";
    let reset = "\x1b[0m";

    // Resolve `${VAR}` placeholders so verify works against the live secret,
    // not the template string stored on disk.
    let resolved = match resolve_env_placeholders(api_key) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("\r{yellow}{e}{reset}");
            return;
        }
    };
    let api_key = resolved.as_str();

    if provider == "codex" {
        eprint!(
            "{dim}{}{reset}",
            crate::messages::verify::VERIFYING_CODEX_LOGIN
        );
        match codex_login_state().await {
            CodexLoginState::Verified => {
                eprintln!(
                    "\r{green}{}{reset}                    ",
                    crate::messages::verify::CODEX_LOGIN_VERIFIED
                );
            }
            CodexLoginState::NotLoggedIn => {
                eprintln!(
                    "\r{yellow}{}{reset}",
                    crate::messages::verify::CODEX_NOT_LOGGED_IN
                );
            }
            CodexLoginState::CheckFailed => {
                eprintln!(
                    "\r{dim}{}{reset}",
                    crate::messages::verify::CODEX_LOGIN_CHECK_FAILED
                );
            }
        }
        return;
    }

    eprint!("{dim}{}{reset}", crate::messages::verify::VERIFYING_KEY);

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => {
            eprintln!(
                "\r{dim}{}{reset}",
                crate::messages::verify::VERIFY_SKIPPED_CLIENT
            );
            return;
        }
    };

    let result = match provider {
        "anthropic" => {
            client
                .get("https://api.anthropic.com/v1/models")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .send()
                .await
        }
        "openai" => {
            client
                .get("https://api.openai.com/v1/models")
                .header("Authorization", format!("Bearer {api_key}"))
                .send()
                .await
        }
        _ => return,
    };

    match result {
        Ok(resp) if resp.status().is_success() => {
            eprintln!(
                "\r{green}{}{reset}                    ",
                crate::messages::verify::KEY_VERIFIED
            );
        }
        Ok(resp) => {
            let status = resp.status().as_u16();
            eprintln!(
                "\r{yellow}{}{reset}",
                crate::messages::verify::key_invalid(status)
            );
        }
        Err(_) => {
            eprintln!(
                "\r{dim}{}{reset}       ",
                crate::messages::verify::VERIFY_SKIPPED_NETWORK
            );
        }
    }
}

/// Masks an API key for display.
pub(super) fn mask_api_key(key: &str) -> String {
    if key.trim().is_empty() {
        "not set".to_string()
    } else if key.len() > 8 {
        format!("{}***", &key[..8])
    } else if key.len() > 4 {
        format!("{}***", &key[..4])
    } else {
        "***".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_auth_method_and_usage_flags() {
        assert_eq!(provider_auth_method("codex"), "Codex login session");
        assert_eq!(provider_auth_method("openai"), "API key");
        assert_eq!(provider_auth_method("anthropic"), "API key");
        assert!(provider_uses_api_key("openai"));
        assert!(provider_uses_api_key("anthropic"));
        assert!(!provider_uses_api_key("codex"));
    }

    #[test]
    fn test_has_stored_api_key_trims_whitespace() {
        assert!(has_stored_api_key("sk-test-key"));
        assert!(!has_stored_api_key(""));
        assert!(!has_stored_api_key("   "));
    }

    #[test]
    fn test_infer_codex_login_state_reads_stderr_logged_in() {
        assert_eq!(
            infer_codex_login_state(true, "", "Logged in using ChatGPT"),
            CodexLoginState::Verified
        );
    }

    #[test]
    fn test_infer_codex_login_state_not_logged_in_phrase_wins() {
        assert_eq!(
            infer_codex_login_state(true, "Logged in", "Not logged in"),
            CodexLoginState::NotLoggedIn
        );
    }
}
