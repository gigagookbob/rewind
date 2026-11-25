// LLM provider abstraction. Supports Anthropic and OpenAI.
// Prompts are loaded from prompts/*.md at compile time via include_str!().

use crate::config::Lang;

const SYSTEM_PROMPT_EN: &str = include_str!("../../prompts/system_en.md");
const SYSTEM_PROMPT_KO: &str = include_str!("../../prompts/system_ko.md");
const DAY_PROMPT_EN: &str = include_str!("../../prompts/day_en.md");
const DAY_PROMPT_KO: &str = include_str!("../../prompts/day_ko.md");
const SLACK_PROMPT_EN: &str = include_str!("../../prompts/slack_en.md");
const SLACK_PROMPT_KO: &str = include_str!("../../prompts/slack_ko.md");

fn get_system_prompt(lang: &Lang) -> &'static str {
    match lang {
        Lang::En => SYSTEM_PROMPT_EN,
        Lang::Ko => SYSTEM_PROMPT_KO,
    }
}

fn get_day_prompt(lang: &Lang) -> &'static str {
    match lang {
        Lang::En => DAY_PROMPT_EN,
        Lang::Ko => DAY_PROMPT_KO,
    }
}

fn get_slack_prompt(lang: &Lang) -> &'static str {
    match lang {
        Lang::En => SLACK_PROMPT_EN,
        Lang::Ko => SLACK_PROMPT_KO,
    }
}

/// LLM provider enum. Adding a new variant triggers compile errors at unhandled match arms.
#[derive(Clone, serde::Serialize)]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
}

pub struct LlmProvider {
    pub(super) kind: ProviderKind,
    redactor_enabled: bool,
    anthropic_workspace_id: Option<String>,
    #[cfg(test)]
    pub(super) recording: Option<super::transport_tests::Recording>,
}

/// The exact object handed to HTTP execution.
pub(super) enum OutboundRequest {
    Anthropic(reqwest::Request),
    OpenAi(reqwest::Request),
}

impl LlmProvider {
    /// No credentials enter checkpoint identity or files.
    pub(super) fn cache_identity(&self, lang: &Lang) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind,
            "anthropic_model": super::anthropic::MODEL,
            "anthropic_output_schema": super::anthropic::analysis_output_schema(),
            "openai_model": super::openai::MODEL,
            "redactor": self.redactor_enabled,
            "workspace": self.anthropic_workspace_id,
            "lang": lang,
            "system": get_system_prompt(lang),
            "chunk_system": super::summarizer::get_chunk_summarize_prompt(lang),
        })
    }

    pub fn new(kind: ProviderKind, redactor_enabled: bool) -> Self {
        Self {
            kind,
            redactor_enabled,
            anthropic_workspace_id: None,
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
        }
    }

    /// Maximum single-request input tokens safely under the model context window.
    pub fn max_input_tokens(&self) -> u64 {
        match &self.kind {
            ProviderKind::Anthropic => 160_000,
            ProviderKind::OpenAi => 100_000,
        }
    }

    /// Calls the insight-extraction API with the system prompt selected by language.
    #[cfg(test)]
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

    /// Calls the insight-extraction API with automatic `max_tokens` expansion
    /// when output is truncated before reaching `max_output_tokens()`.
    pub(super) async fn call_analysis_api_with_retry(
        &self,
        api_key: &str,
        conversation_text: &str,
        initial_max_tokens: u32,
        lang: &Lang,
        input_budget: u64,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        self.call_api_with_truncation_retry(
            api_key,
            get_system_prompt(lang),
            conversation_text,
            initial_max_tokens,
            true,
            input_budget,
        )
        .await
    }

    /// Calls the API for the private day digest placed on top of the daily note.
    pub async fn call_day_summary_api(
        &self,
        api_key: &str,
        session_summaries: &str,
        lang: &Lang,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        self.call_text_api_with_retry(
            api_key,
            get_day_prompt(lang),
            session_summaries,
            4_096,
            self.max_input_tokens(),
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
        self.call_text_api_with_retry(
            api_key,
            get_slack_prompt(lang),
            session_summaries,
            4_096,
            self.max_input_tokens(),
        )
        .await
    }

    /// Calls the API with an arbitrary system prompt for plain-text output
    /// (day summary, Slack message, chunk summarization).
    #[cfg(test)]
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

    /// Calls `call_text_api` with automatic `max_tokens` expansion on truncation.
    pub(super) async fn call_text_api_with_retry(
        &self,
        api_key: &str,
        system_prompt: &str,
        conversation_text: &str,
        initial_max_tokens: u32,
        input_budget: u64,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        self.call_api_with_truncation_retry(
            api_key,
            system_prompt,
            conversation_text,
            initial_max_tokens,
            false,
            input_budget,
        )
        .await
    }

    async fn call_api_with_truncation_retry(
        &self,
        api_key: &str,
        system: &str,
        text: &str,
        initial_max_tokens: u32,
        json_output: bool,
        input_budget: u64,
    ) -> Result<(String, super::ApiUsage), super::AnalyzerError> {
        let cap = u32::try_from(self.max_output_tokens())?;
        let mut max_tokens = initial_max_tokens.min(cap);
        let mut total_usage = super::ApiUsage::default();
        loop {
            match self
                .call_api(api_key, system, text, max_tokens, json_output, input_budget)
                .await
            {
                Ok((raw_response, mut usage)) => {
                    usage.input_tokens += total_usage.input_tokens;
                    usage.output_tokens += total_usage.output_tokens;
                    return Ok((raw_response, usage));
                }
                Err(error) => {
                    let Some(truncated) = error.downcast_ref::<super::OutputTruncated>() else {
                        return Err(error);
                    };
                    if max_tokens >= cap {
                        return Err(error);
                    }
                    total_usage.input_tokens += truncated.usage.input_tokens;
                    total_usage.output_tokens += truncated.usage.output_tokens;
                    let next = max_tokens.saturating_mul(2).min(cap);
                    eprintln!(
                        "LLM output truncated; retrying with max_tokens {max_tokens} -> {next}"
                    );
                    max_tokens = next;
                }
            }
        }
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
                api_key,
                self.anthropic_workspace_id.as_deref(),
                &system,
                &text,
                max_tokens,
                json_output,
            )?),
            ProviderKind::OpenAi => OutboundRequest::OpenAi(super::openai::build_request(
                api_key,
                &system,
                &text,
                max_tokens,
                json_output,
            )?),
        };
        #[cfg(test)]
        if let Some(recording) = &self.recording {
            return recording.send(request);
        }
        match request {
            OutboundRequest::Anthropic(request) => super::anthropic::send_request(request).await,
            OutboundRequest::OpenAi(request) => super::openai::send_request(request).await,
        }
    }

    /// Check complete, already masked input. Never shorten it to fit.
    pub(super) fn check_input(
        &self,
        system: &str,
        text: &str,
        budget: u64,
    ) -> Result<(), super::AnalyzerError> {
        if self.estimate_input_tokens(system, text) > budget {
            return Err(
                "AI input exceeds the input budget; no conversation text was truncated".into(),
            );
        }
        Ok(())
    }

    pub(super) fn estimate_input_tokens(&self, system: &str, text: &str) -> u64 {
        super::prompt::estimate_tokens(&format!("{system}\n{text}"))
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
        }
    }

    /// Probes actual rate limits via a lightweight API call.
    /// Returns (RateLimits, probed) where probed=true means real values, false means defaults.
    pub async fn probe_rate_limits(
        &self,
        api_key: &str,
    ) -> Result<(super::planner::RateLimits, bool), super::AnalyzerError> {
        let result = match &self.kind {
            ProviderKind::Anthropic => {
                super::anthropic::probe_anthropic_rate_limits(
                    api_key,
                    self.anthropic_workspace_id.as_deref(),
                )
                .await?
            }
            ProviderKind::OpenAi => super::openai::probe_openai_rate_limits(api_key).await,
        };
        Ok(match result {
            Some(limits) => (limits, true),
            None => (super::planner::RateLimits::default_generous(), false),
        })
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
            return Err(crate::messages::error::CODEX_PROVIDER_REMOVED.into());
        }
        "anthropic" => (
            ProviderKind::Anthropic,
            resolve(&config.llm.anthropic_api_key)?,
        ),
        _ => {
            return Err(crate::messages::error::unsupported_provider_in_config().into());
        }
    };
    let mut provider = LlmProvider::new(provider, config.is_redactor_enabled());
    if matches!(provider.kind, ProviderKind::Anthropic) {
        provider.anthropic_workspace_id = config.llm.anthropic_workspace_id()?;
    }
    Ok((provider, api_key))
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
