//! Provider authentication status, verification, and masked key display.

use super::{ConfigError, LlmConfig, config_path, load_config, resolve_env_placeholders};

pub(super) fn provider_auth_method(_provider: &str) -> &'static str {
    "API key"
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

    if provider_uses_api_key(new_provider)
        && !has_stored_api_key(llm.api_key_for_provider(new_provider))
    {
        eprintln!(
            "{}",
            crate::messages::config::provider_requires_api_key(new_provider)
        );
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
    if provider == "anthropic" {
        println!(
            "Anthropic workspace: {}",
            config
                .llm
                .anthropic_workspace_id()?
                .as_deref()
                .unwrap_or("not set (workspace-scoped key required)")
        );
    }

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
        println!("{}", crate::messages::error::CODEX_PROVIDER_REMOVED);
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
pub(super) async fn verify_api_key(provider: &str, api_key: &str, workspace_id: Option<&str>) {
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
        "anthropic" => match anthropic_verification_request(&client, api_key, workspace_id) {
            Ok(request) => client.execute(request).await,
            Err(error) => {
                eprintln!("\r{yellow}{error}{reset}");
                return;
            }
        },
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
            let message = if provider == "anthropic" {
                crate::analyzer::anthropic_error::describe(resp).await
            } else {
                crate::messages::verify::verification_failed(status)
            };
            eprintln!("\r{yellow}{message}{reset}");
        }
        Err(_) => {
            eprintln!(
                "\r{dim}{}{reset}       ",
                crate::messages::verify::VERIFY_SKIPPED_NETWORK
            );
        }
    }
}

fn anthropic_verification_request(
    client: &reqwest::Client,
    api_key: &str,
    workspace_id: Option<&str>,
) -> Result<reqwest::Request, ConfigError> {
    let request = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01");
    Ok(crate::analyzer::anthropic::with_workspace(request, workspace_id)?.build()?)
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
    fn test_anthropic_verification_uses_selected_workspace() {
        let client = reqwest::Client::new();
        for workspace in [None, Some("wrkspc_test123")] {
            let request = anthropic_verification_request(&client, "synthetic", workspace).unwrap();
            assert_eq!(request.method(), reqwest::Method::GET);
            assert_eq!(
                request.url().as_str(),
                "https://api.anthropic.com/v1/models"
            );
            assert_eq!(
                request
                    .headers()
                    .get("anthropic-workspace-id")
                    .map(|v| v.to_str().unwrap()),
                workspace
            );
        }
        assert!(
            anthropic_verification_request(
                &client,
                "synthetic",
                Some("wrkspc_test\nInjected: true")
            )
            .is_err()
        );
    }

    #[test]
    fn test_provider_auth_method_and_usage_flags() {
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
}
