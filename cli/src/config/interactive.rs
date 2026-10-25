//! Interactive configuration menu and cancellable terminal input.

use super::auth::{
    mask_api_key, print_provider_switch_guidance, provider_auth_method, verify_api_key,
};
use super::{
    Config, ConfigError, DEFAULT_CODEX_MODEL, DEFAULT_CODEX_REASONING_EFFORT, Lang, RedactorConfig,
    config_path, load_config, parse_codex_model_value, parse_reasoning_effort, save_config,
};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Confirm, Select};

/// Reads a line of terminal input with Esc-to-cancel support.
///
/// Esc returns `None` (cancel), Enter returns the typed input. When `mask` is
/// set, typed characters echo as `*` (password mode). When a `default` is
/// given, Enter on empty input returns it.
fn read_line_with_esc(
    prompt: &str,
    default: Option<&str>,
    mask: bool,
) -> Result<Option<String>, ConfigError> {
    use console::{Key, Term};
    let term = Term::stderr();
    eprint!("{prompt}");
    let mut input = String::new();
    loop {
        match term.read_key()? {
            Key::Escape => {
                term.write_line("")?;
                return Ok(None);
            }
            Key::Enter => {
                term.write_line("")?;
                let value = match default {
                    Some(default) if input.is_empty() => default.to_string(),
                    _ => input,
                };
                return Ok(Some(value));
            }
            Key::Backspace => {
                if !input.is_empty() {
                    input.pop();
                    term.clear_chars(1)?;
                }
            }
            Key::Char(c) if mask => {
                input.push(c);
                eprint!("*");
            }
            Key::Char(c) if !c.is_ascii_control() => {
                input.push(c);
                eprint!("{c}");
            }
            _ => {}
        }
    }
}

fn read_input_with_esc(prompt: &str, default: &str) -> Result<Option<String>, ConfigError> {
    read_line_with_esc(prompt, Some(default), false)
}

fn read_password_with_esc(prompt: &str) -> Result<Option<String>, ConfigError> {
    read_line_with_esc(prompt, None, true)
}

/// Interactive flow for changing a provider API key: prompt, confirm, save, verify.
/// Returns without changes when the user cancels at any step.
async fn prompt_provider_api_key(
    config: &mut Config,
    config_file: &std::path::Path,
    theme: &ColorfulTheme,
    provider: &str,
    provider_label: &str,
) -> Result<(), ConfigError> {
    let green = "\x1b[32m";
    let reset = "\x1b[0m";

    let key_prompt = crate::messages::config::new_provider_api_key(provider_label);
    let Some(new_key) = read_password_with_esc(&key_prompt)? else {
        return Ok(());
    };
    let new_key = new_key.trim().to_string();

    if new_key.is_empty() {
        return Ok(());
    }
    let confirmed = Confirm::with_theme(theme)
        .with_prompt(crate::messages::config::confirm_provider_api_key(
            provider_label,
        ))
        .default(false)
        .interact_opt()?;
    if confirmed != Some(true) {
        return Ok(());
    }
    let old_masked = mask_api_key(config.llm.api_key_for_provider(provider));
    config.llm.set_api_key_for_provider(provider, new_key);
    save_config(config, config_file)?;
    eprintln!(
        "{green}{}{reset}",
        crate::messages::config::changed(
            &old_masked,
            &mask_api_key(config.llm.api_key_for_provider(provider))
        )
    );
    verify_api_key(provider, config.llm.api_key_for_provider(provider)).await;
    eprintln!();
    Ok(())
}

/// `rwd config` (no args) — interactive menu for changing settings.
pub(crate) async fn run_config_interactive() -> Result<(), ConfigError> {
    let config_file = config_path()?;

    if !config_file.exists() {
        return Err(crate::messages::config::NO_CONFIG.into());
    }

    let mut config = load_config(&config_file)?;
    let theme = ColorfulTheme::default();

    let cyan = "\x1b[36m";
    let dim = "\x1b[2m";
    let reset = "\x1b[0m";

    eprintln!("{dim}{}{reset}", crate::messages::config::NAV_HINT);

    loop {
        let redactor_status = if config.is_redactor_enabled() {
            "on"
        } else {
            "off"
        };
        let codex_model = config
            .llm
            .codex_model
            .as_deref()
            .unwrap_or(DEFAULT_CODEX_MODEL);
        let codex_reasoning = config
            .llm
            .codex_reasoning_effort
            .as_deref()
            .unwrap_or(DEFAULT_CODEX_REASONING_EFFORT);
        let openai_detail = if config.llm.provider == "openai" {
            "active"
        } else {
            "inactive"
        };
        let anthropic_detail = if config.llm.provider == "anthropic" {
            "active"
        } else {
            "inactive"
        };
        let items = vec![
            format!(
                "{cyan}provider{reset}      {dim}[{}]{reset}",
                config.llm.provider
            ),
            format!(
                "{cyan}openai-api-key{reset}    {dim}[{} ({openai_detail})]{reset}",
                mask_api_key(&config.llm.openai_api_key)
            ),
            format!(
                "{cyan}anthropic-api-key{reset} {dim}[{} ({anthropic_detail})]{reset}",
                mask_api_key(&config.llm.anthropic_api_key)
            ),
            format!("{cyan}codex-model{reset}   {dim}[{codex_model}]{reset}"),
            format!("{cyan}codex-reasoning{reset} {dim}[{codex_reasoning}]{reset}"),
            format!(
                "{cyan}output-path{reset}   {dim}[{}]{reset}",
                config.output.path
            ),
            format!(
                "{cyan}redactor{reset}      {dim}[{}]{reset}",
                redactor_status
            ),
            format!(
                "{cyan}lang{reset}          {dim}[{}]{reset}",
                config.lang.as_ref().map_or("not set", |l| match l {
                    Lang::En => "en",
                    Lang::Ko => "ko",
                })
            ),
            format!("{dim}{}{reset}", crate::messages::config::EXIT),
        ];

        let selection = Select::with_theme(&theme)
            .with_prompt(crate::messages::config::SELECT_SETTING)
            .items(&items)
            .default(0)
            .interact_opt()?;

        let Some(selection) = selection else { break };

        let green = "\x1b[32m";
        let yellow = "\x1b[33m";

        match selection {
            // provider
            0 => {
                let old = config.llm.provider.clone();
                let providers = ["anthropic", "openai", "codex"];
                let current_idx = providers.iter().position(|&p| p == old).unwrap_or(0);

                let Some(chosen) = Select::with_theme(&theme)
                    .with_prompt(crate::messages::config::LLM_PROVIDER)
                    .items(&providers)
                    .default(current_idx)
                    .interact_opt()?
                else {
                    continue;
                };

                let new_provider = providers[chosen];
                if new_provider == old {
                    eprintln!("{dim}{}{reset}\n", crate::messages::config::NO_CHANGE);
                } else {
                    config.llm.provider = new_provider.to_string();
                    save_config(&config, &config_file)?;
                    eprintln!(
                        "{green}{}{reset}",
                        crate::messages::config::changed(&old, new_provider)
                    );
                    eprintln!(
                        "{dim}{}{reset}",
                        crate::messages::config::provider_now_uses(
                            new_provider,
                            provider_auth_method(new_provider),
                        )
                    );
                    print_provider_switch_guidance(&old, new_provider, &config.llm);
                    verify_api_key(
                        &config.llm.provider,
                        config.llm.api_key_for_provider(&config.llm.provider),
                    )
                    .await;
                    eprintln!();
                }
            }
            // openai-api-key
            1 => {
                prompt_provider_api_key(&mut config, &config_file, &theme, "openai", "OpenAI")
                    .await?;
            }
            // anthropic-api-key
            2 => {
                prompt_provider_api_key(
                    &mut config,
                    &config_file,
                    &theme,
                    "anthropic",
                    "Anthropic",
                )
                .await?;
            }
            // codex-model
            3 => {
                let old_effective = config
                    .llm
                    .codex_model
                    .as_deref()
                    .unwrap_or(DEFAULT_CODEX_MODEL)
                    .to_string();
                let prompt = format!("  Codex model ({old_effective}): ");
                let Some(new_value) = read_input_with_esc(&prompt, &old_effective)? else {
                    continue;
                };
                let parsed = parse_codex_model_value(&new_value);
                let new_effective = parsed.as_deref().unwrap_or(DEFAULT_CODEX_MODEL).to_string();

                if new_effective == old_effective {
                    eprintln!("{dim}{}{reset}\n", crate::messages::config::NO_CHANGE);
                } else {
                    config.llm.codex_model = parsed;
                    save_config(&config, &config_file)?;
                    eprintln!(
                        "{green}{}{reset}\n",
                        crate::messages::config::changed(&old_effective, &new_effective)
                    );
                }
            }
            // codex-reasoning
            4 => {
                let old_effective = config
                    .llm
                    .codex_reasoning_effort
                    .as_deref()
                    .unwrap_or(DEFAULT_CODEX_REASONING_EFFORT)
                    .to_string();
                let prompt = format!(
                    "  Codex reasoning effort ({old_effective}) [low/medium/high/xhigh/default]: "
                );
                let Some(new_value) = read_input_with_esc(&prompt, &old_effective)? else {
                    continue;
                };
                let parsed = match parse_reasoning_effort(&new_value) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{yellow}{e}{reset}\n");
                        continue;
                    }
                };
                let new_effective = parsed
                    .as_deref()
                    .unwrap_or(DEFAULT_CODEX_REASONING_EFFORT)
                    .to_string();

                if new_effective == old_effective {
                    eprintln!("{dim}{}{reset}\n", crate::messages::config::NO_CHANGE);
                } else {
                    config.llm.codex_reasoning_effort = parsed;
                    save_config(&config, &config_file)?;
                    eprintln!(
                        "{green}{}{reset}\n",
                        crate::messages::config::changed(&old_effective, &new_effective)
                    );
                }
            }
            // output-path
            5 => {
                let old = config.output.path.clone();
                let prompt = format!("  {} ({old}): ", crate::messages::config::OUTPUT_PATH);
                let Some(new_path) = read_input_with_esc(&prompt, &old)? else {
                    continue;
                };

                if new_path == old {
                    eprintln!("{dim}{}{reset}\n", crate::messages::config::NO_CHANGE);
                } else {
                    config.output.path = new_path.clone();
                    save_config(&config, &config_file)?;
                    eprintln!(
                        "{green}{}{reset}\n",
                        crate::messages::config::changed(&old, &new_path)
                    );
                }
            }
            // redactor
            6 => {
                let old_enabled = config.is_redactor_enabled();
                let options = ["on", "off"];
                let current_idx = if old_enabled { 0 } else { 1 };

                let Some(chosen) = Select::with_theme(&theme)
                    .with_prompt(crate::messages::config::REDACTOR)
                    .items(&options)
                    .default(current_idx)
                    .interact_opt()?
                else {
                    continue;
                };

                let enabled = chosen == 0;
                if enabled == old_enabled {
                    eprintln!("{dim}{}{reset}\n", crate::messages::config::NO_CHANGE);
                } else {
                    config.redactor = Some(RedactorConfig { enabled });
                    save_config(&config, &config_file)?;
                    let old_label = if old_enabled { "on" } else { "off" };
                    let new_label = options[chosen];
                    eprintln!(
                        "{green}{}{reset}\n",
                        crate::messages::config::changed(old_label, new_label)
                    );
                }
            }
            // lang
            7 => {
                let langs = ["en", "ko"];
                let current_idx = config.lang.as_ref().map_or(0, |l| match l {
                    Lang::En => 0,
                    Lang::Ko => 1,
                });

                let Some(chosen) = Select::with_theme(&theme)
                    .with_prompt(crate::messages::config::LANGUAGE)
                    .items(&langs)
                    .default(current_idx)
                    .interact_opt()?
                else {
                    continue;
                };

                let new_lang = if chosen == 0 { Lang::En } else { Lang::Ko };
                let old_label = config
                    .lang
                    .as_ref()
                    .map_or("not set".to_string(), |l| l.to_string());
                if config.lang.as_ref() == Some(&new_lang) {
                    eprintln!("{dim}{}{reset}\n", crate::messages::config::NO_CHANGE);
                } else {
                    config.lang = Some(new_lang.clone());
                    save_config(&config, &config_file)?;
                    eprintln!(
                        "{green}{}{reset}\n",
                        crate::messages::config::changed(&old_label, &new_lang.to_string())
                    );
                }
            }
            // exit
            8 => {
                console::Term::stderr().clear_last_lines(1)?;
                break;
            }
            _ => unreachable!(),
        }
    }

    eprintln!(
        "\x1b[32m{}\x1b[0m",
        crate::messages::config::config_saved(&config_file.display())
    );
    Ok(())
}
