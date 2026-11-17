//! Initial setup and non-interactive configuration updates.

use super::auth::{
    mask_api_key, print_provider_switch_guidance, provider_auth_method, provider_uses_api_key,
};
use super::vault::{default_output_path, detect_obsidian_vault};
use super::{
    ClaudeInputConfig, Config, ConfigError, InputConfig, Lang, LlmConfig, OutputConfig,
    config_path, load_config, parse_bool_toggle, save_config,
};
use std::path::PathBuf;

/// `rwd init` — prompts for API key, detects output path, and saves config.
pub(crate) fn run_init() -> Result<(), ConfigError> {
    let config_file = config_path()?;

    // Provider selection
    eprint!("{}", crate::messages::init::SELECT_PROVIDER);
    let mut provider_input = String::new();
    std::io::stdin().read_line(&mut provider_input)?;
    let provider = provider_input.trim();
    let provider = if provider.is_empty() {
        "anthropic"
    } else {
        provider
    };

    if !["anthropic", "openai"].contains(&provider) {
        return Err(crate::messages::init::unsupported_provider(provider).into());
    }

    // API key input (masked).
    let key_prompt = if provider == "openai" {
        crate::messages::init::ENTER_API_KEY_OPENAI
    } else {
        crate::messages::init::ENTER_API_KEY_ANTHROPIC
    };
    let entered_api_key = rpassword::prompt_password(key_prompt)
        .map_err(|e| crate::messages::init::api_key_input_failed(&e))?
        .trim()
        .to_string();

    if entered_api_key.is_empty() {
        return Err(crate::messages::init::API_KEY_EMPTY.into());
    }

    // Show masked key (first 8 chars + ***)
    let masked = if entered_api_key.len() > 8 {
        format!("{}***", &entered_api_key[..8])
    } else {
        "***".to_string()
    };
    eprintln!("{}", crate::messages::init::api_key_set(&masked));

    // Output path — suggest detected vault path as default
    let default_path = detect_obsidian_vault().unwrap_or_else(default_output_path);
    eprint!(
        "{}",
        crate::messages::init::output_path_prompt(&default_path.display())
    );
    let mut path_input = String::new();
    std::io::stdin().read_line(&mut path_input)?;
    let path_input = path_input.trim();
    let output_path = if path_input.is_empty() {
        default_path
    } else {
        PathBuf::from(path_input)
    };
    eprintln!(
        "{}",
        crate::messages::init::output_path_set(&output_path.display())
    );

    // Language selection
    eprint!("{}", crate::messages::lang::SELECT);
    let mut lang_input = String::new();
    std::io::stdin().read_line(&mut lang_input)?;
    let lang = match lang_input.trim() {
        "ko" => Lang::Ko,
        _ => Lang::En,
    };

    let config = Config {
        llm: LlmConfig {
            provider: provider.to_string(),
            openai_api_key: if provider == "openai" {
                entered_api_key.clone()
            } else {
                String::new()
            },
            anthropic_api_key: if provider == "anthropic" {
                entered_api_key
            } else {
                String::new()
            },
            anthropic_workspace_id: None,
            legacy_api_key: String::new(),
        },
        output: OutputConfig {
            path: output_path.to_string_lossy().to_string(),
        },
        redactor: None,
        lang: Some(lang),
        input: None,
    };

    save_config(&config, &config_file)?;
    eprintln!(
        "{}",
        crate::messages::init::config_saved(&config_file.display())
    );
    Ok(())
}

/// `rwd config <key> <value>` — updates a single config value.
pub(crate) fn run_config(key: &str, value: &str) -> Result<(), ConfigError> {
    let config_file = config_path()?;

    if !config_file.exists() {
        return Err(crate::messages::config::NO_CONFIG.into());
    }

    let mut config = load_config(&config_file)?;
    let old_provider = config.llm.provider.clone();

    match key {
        "output-path" => {
            config.output.path = value.to_string();
            eprintln!("{}", crate::messages::config::output_path_changed(value));
        }
        "provider" => {
            if !["anthropic", "openai"].contains(&value) {
                return Err(crate::messages::config::unsupported_provider(value).into());
            }
            config.llm.provider = value.to_string();
            eprintln!(
                "{}",
                crate::messages::config::provider_changed(value, provider_auth_method(value))
            );
            print_provider_switch_guidance(&old_provider, value, &config.llm);
        }
        "api-key" => {
            if !provider_uses_api_key(&config.llm.provider) {
                return Err(crate::messages::config::api_key_unused_for_provider(
                    &config.llm.provider,
                )
                .into());
            }
            let provider = config.llm.provider.clone();
            config
                .llm
                .set_api_key_for_provider(&provider, value.to_string());
            eprintln!(
                "{}",
                crate::messages::config::api_key_changed(&mask_api_key(value))
            );
        }
        "openai-api-key" => {
            config.llm.openai_api_key = value.to_string();
            eprintln!(
                "{}",
                crate::messages::config::provider_api_key_changed("OpenAI", &mask_api_key(value))
            );
        }
        "anthropic-api-key" => {
            config.llm.anthropic_api_key = value.to_string();
            eprintln!(
                "{}",
                crate::messages::config::provider_api_key_changed(
                    "Anthropic",
                    &mask_api_key(value),
                )
            );
        }
        "anthropic-workspace-id" => {
            config.llm.anthropic_workspace_id = super::parse_anthropic_workspace_id(value)?;
            eprintln!(
                "Anthropic workspace: {}",
                config
                    .llm
                    .anthropic_workspace_id
                    .as_deref()
                    .unwrap_or("not set")
            );
        }
        "lang" => {
            let lang = match value {
                "ko" => Lang::Ko,
                "en" => Lang::En,
                _ => return Err(crate::messages::lang::unsupported(value).into()),
            };
            config.lang = Some(lang);
            eprintln!("Language changed: {value}");
        }
        "claude-include-automated" => {
            let enabled = parse_bool_toggle(value, "claude-include-automated")?;
            let input = config.input.get_or_insert_with(InputConfig::default);
            let claude = input.claude.get_or_insert(ClaudeInputConfig {
                roots: None,
                include_automated: false,
            });
            claude.include_automated = enabled;
            eprintln!(
                "{}",
                crate::messages::config::claude_include_automated_changed(enabled)
            );
        }
        _ => {
            return Err(crate::messages::config::unknown_key(key).into());
        }
    }

    save_config(&config, &config_file)?;
    Ok(())
}
