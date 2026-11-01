// LLM provider abstraction. Supports Anthropic, OpenAI, and Codex CLI.
// Prompts are loaded from prompts/*.md at compile time via include_str!().

use crate::config::Lang;

const SYSTEM_PROMPT_EN: &str = include_str!("../../prompts/system_en.md");
const SYSTEM_PROMPT_KO: &str = include_str!("../../prompts/system_ko.md");
const SUMMARY_PROMPT_EN: &str = include_str!("../../prompts/summary_en.md");
const SUMMARY_PROMPT_KO: &str = include_str!("../../prompts/summary_ko.md");
const SLACK_PROMPT_EN: &str = include_str!("../../prompts/slack_en.md");
const SLACK_PROMPT_KO: &str = include_str!("../../prompts/slack_ko.md");
const CODEX_PROMPT_PREFIX: &str = "[System Instructions]\n";
const CODEX_PROMPT_MIDDLE: &str = "\n\n[Conversation]\n";
const CODEX_MAX_INPUT_CHARS: usize = 1_048_576;
const CODEX_INPUT_SAFETY_MARGIN_CHARS: usize = 8_192;

fn get_system_prompt(lang: &Lang) -> &'static str {
    match lang {
        Lang::En => SYSTEM_PROMPT_EN,
        Lang::Ko => SYSTEM_PROMPT_KO,
    }
}

fn get_summary_prompt(lang: &Lang) -> &'static str {
    match lang {
        Lang::En => SUMMARY_PROMPT_EN,
        Lang::Ko => SUMMARY_PROMPT_KO,
    }
}

fn get_slack_prompt(lang: &Lang) -> &'static str {
    match lang {
        Lang::En => SLACK_PROMPT_EN,
        Lang::Ko => SLACK_PROMPT_KO,
    }
}

fn codex_max_conversation_chars(system_prompt: &str) -> usize {
    let overhead = CODEX_PROMPT_PREFIX.chars().count()
        + CODEX_PROMPT_MIDDLE.chars().count()
        + system_prompt.chars().count()
        + CODEX_INPUT_SAFETY_MARGIN_CHARS;
    CODEX_MAX_INPUT_CHARS.saturating_sub(overhead)
}

/// LLM provider enum. Adding a new variant triggers compile errors at unhandled match arms.
#[derive(Clone)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Codex {
        model: String,
        reasoning_effort: String,
    },
}

pub struct LlmProvider {
    pub(super) kind: ProviderKind,
    redactor_enabled: bool,
    #[cfg(test)]
    pub(super) recording: Option<super::transport_tests::Recording>,
}

/// The exact object handed to HTTP execution or the Codex stdin writer.
pub(super) enum OutboundRequest {
    Anthropic(reqwest::Request),
    OpenAi(reqwest::Request),
    Codex {
        stdin: String,
        max_tokens: u32,
        model: String,
        reasoning_effort: String,
        json_output: bool,
    },
}

impl LlmProvider {
    pub fn new(kind: ProviderKind, redactor_enabled: bool) -> Self {
        Self {
            kind,
            redactor_enabled,
            #[cfg(test)]
            recording: None,
        }
    }

    pub fn redactor_enabled(&self) -> bool {
        self.redactor_enabled
    }
    /// Maximum output tokens for the model (provider-specific constant).
    pub fn max_output_tokens(&self) -> u64 {
        match &self.kind {
            ProviderKind::Anthropic => 32_000,
            ProviderKind::OpenAi => 16_384,
            ProviderKind::Codex { .. } => 16_384,
        }
    }

    /// Returns a safe max length for conversation text for an arbitrary system prompt.
    pub fn max_conversation_chars_with_prompt(&self, system_prompt: &str) -> Option<usize> {
        match &self.kind {
            ProviderKind::Codex { .. } => Some(codex_max_conversation_chars(system_prompt)),
            _ => None,
        }
    }

    /// Calls the insight-extraction API with the system prompt selected by language.
    /// Codex enforces the structured JSON output schema on this path.
    pub async fn call_analysis_api(
        &self,
        api_key: &str,
        conversation_text: &str,
        max_tokens: u32,
        lang: &Lang,
        input_budget: u64,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        self.call_api(
            api_key,
            get_system_prompt(lang),
            conversation_text,
            max_tokens,
            true,
            input_budget,
        )
        .await
    }

    /// Calls the summary API with language-selected prompt.
    pub async fn call_summary_api(
        &self,
        api_key: &str,
        session_summaries: &str,
        lang: &Lang,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        self.call_text_api(
            api_key,
            get_summary_prompt(lang),
            session_summaries,
            16_384,
            super::planner::RateLimits::default_generous().input_tokens_per_minute,
        )
        .await
    }

    /// Calls the Slack message API with language-selected prompt.
    pub async fn call_slack_api(
        &self,
        api_key: &str,
        session_summaries: &str,
        lang: &Lang,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        self.call_text_api(
            api_key,
            get_slack_prompt(lang),
            session_summaries,
            4_096,
            super::planner::RateLimits::default_generous().input_tokens_per_minute,
        )
        .await
    }

    /// Calls the API with an arbitrary system prompt for plain-text output
    /// (summary, Slack message, chunk summarization).
    pub async fn call_text_api(
        &self,
        api_key: &str,
        system_prompt: &str,
        conversation_text: &str,
        max_tokens: u32,
        input_budget: u64,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        self.call_api(
            api_key,
            system_prompt,
            conversation_text,
            max_tokens,
            false,
            input_budget,
        )
        .await
    }

    async fn call_api(
        &self,
        api_key: &str,
        system: &str,
        text: &str,
        max_tokens: u32,
        json_output: bool,
        input_budget: u64,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        let (system, text) = if self.redactor_enabled {
            let (mut parts, _) = crate::redactor::redact_parts(&[system, text]);
            let text = parts.pop().ok_or("Missing conversation input")?;
            let system = parts.pop().ok_or("Missing system input")?;
            (system, text)
        } else {
            (system.to_string(), text.to_string())
        };
        self.check_input(&system, &text, input_budget)?;
        let request = match &self.kind {
            ProviderKind::Anthropic => OutboundRequest::Anthropic(super::anthropic::build_request(
                api_key, &system, &text, max_tokens,
            )?),
            ProviderKind::OpenAi => OutboundRequest::OpenAi(super::openai::build_request(
                api_key,
                &system,
                &text,
                max_tokens,
                json_output,
            )?),
            ProviderKind::Codex {
                model,
                reasoning_effort,
            } => OutboundRequest::Codex {
                stdin: super::codex_exec::compose_prompt(&system, &text),
                max_tokens,
                model: model.clone(),
                reasoning_effort: reasoning_effort.clone(),
                json_output,
            },
        };
        #[cfg(test)]
        if let Some(recording) = &self.recording {
            return recording.send(request);
        }
        match request {
            OutboundRequest::Anthropic(request) => super::anthropic::send_request(request).await,
            OutboundRequest::OpenAi(request) => super::openai::send_request(request).await,
            OutboundRequest::Codex {
                stdin,
                max_tokens,
                model,
                reasoning_effort,
                json_output,
            } => {
                super::codex_exec::call_codex_api(
                    &stdin,
                    max_tokens,
                    &model,
                    &reasoning_effort,
                    json_output,
                )
                .await
            }
        }
    }

    /// Check complete, already masked input. Never shorten it to fit.
    pub(super) fn check_input(
        &self,
        system: &str,
        text: &str,
        budget: u64,
    ) -> Result<(), super::AnalyzerError> {
        let codex_too_long = matches!(self.kind, ProviderKind::Codex { .. })
            && super::codex_exec::compose_prompt(system, text)
                .chars()
                .count()
                .saturating_add(CODEX_INPUT_SAFETY_MARGIN_CHARS)
                > CODEX_MAX_INPUT_CHARS;
        if codex_too_long || self.estimate_input_tokens(system, text) > budget {
            return Err(
                "AI input exceeds the input budget; no conversation text was truncated".into(),
            );
        }
        Ok(())
    }

    pub(super) fn estimate_input_tokens(&self, system: &str, text: &str) -> u64 {
        let input = match self.kind {
            ProviderKind::Codex { .. } => super::codex_exec::compose_prompt(system, text),
            _ => format!("{system}\n{text}"),
        };
        super::prompt::estimate_tokens(&input)
    }

    pub(super) fn check_analysis_input(
        &self,
        text: &str,
        budget: u64,
        lang: &Lang,
    ) -> Result<(), super::AnalyzerError> {
        self.check_input(get_system_prompt(lang), text, budget)
    }

    /// Returns the display name for this provider.
    pub fn display_name(&self) -> &'static str {
        match &self.kind {
            ProviderKind::Anthropic => "Claude",
            ProviderKind::OpenAi => "OpenAI",
            ProviderKind::Codex { .. } => "Codex",
        }
    }

    /// Probes actual rate limits via a lightweight API call.
    /// Returns (RateLimits, probed) where probed=true means real values, false means defaults.
    pub async fn probe_rate_limits(&self, api_key: &str) -> (super::planner::RateLimits, bool) {
        let result = match &self.kind {
            ProviderKind::Anthropic => super::anthropic::probe_anthropic_rate_limits(api_key).await,
            ProviderKind::OpenAi => super::openai::probe_openai_rate_limits(api_key).await,
            ProviderKind::Codex { .. } => None,
        };
        match result {
            Some(limits) => (limits, true),
            None => (super::planner::RateLimits::default_generous(), false),
        }
    }

    /// Returns true if this provider supports rate-limit probing.
    pub fn supports_rate_limit_probe(&self) -> bool {
        !matches!(self.kind, ProviderKind::Codex { .. })
    }
}

/// Loads the LLM provider and API key from config (~/.config/rwd/config.toml).
///
/// `${VAR}` placeholders in API key fields are resolved from the process
/// environment, so `openai_api_key = "${OPENAI_API_KEY}"` keeps the secret
/// out of config.toml.
pub fn load_provider() -> Result<(LlmProvider, String), super::AnalyzerError> {
    load_provider_from_path(&crate::config::config_path()?)
}

pub(super) fn load_provider_from_path(
    path: &std::path::Path,
) -> Result<(LlmProvider, String), super::AnalyzerError> {
    // Do not expose TOML parser errors: they can contain the offending secret.
    let config = crate::config::load_config(path)
        .map_err(|_| "Cannot load AI provider configuration; no request was sent")?;

    let resolve = |raw: &str| -> Result<String, super::AnalyzerError> {
        crate::config::resolve_env_placeholders(raw).map_err(|e| e.to_string().into())
    };

    let (provider, api_key) = match config.llm.provider.as_str() {
        "openai" => (ProviderKind::OpenAi, resolve(&config.llm.openai_api_key)?),
        "codex" => {
            let model = config
                .llm
                .codex_model
                .as_deref()
                .unwrap_or(crate::config::DEFAULT_CODEX_MODEL)
                .to_string();
            let reasoning_effort = config
                .llm
                .codex_reasoning_effort
                .as_deref()
                .unwrap_or(crate::config::DEFAULT_CODEX_REASONING_EFFORT)
                .to_string();
            (
                ProviderKind::Codex {
                    model,
                    reasoning_effort,
                },
                String::new(),
            )
        }
        "anthropic" => (
            ProviderKind::Anthropic,
            resolve(&config.llm.anthropic_api_key)?,
        ),
        _ => {
            return Err(crate::messages::error::unsupported_provider_in_config().into());
        }
    };
    Ok((
        LlmProvider::new(provider, config.is_redactor_enabled()),
        api_key,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analysis_prompt_estimate_covers_both_languages() {
        for lang in [Lang::Ko, Lang::En] {
            assert!(
                super::super::prompt::estimate_tokens(get_system_prompt(&lang))
                    <= super::super::prompt::SYSTEM_PROMPT_ESTIMATED_TOKENS
            );
        }
    }
}
