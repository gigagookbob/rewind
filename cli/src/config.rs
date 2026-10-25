//! Configuration data, persistence, legacy migration, and shared value parsing.

mod auth;
mod commands;
mod interactive;
mod reset;
mod vault;

pub(crate) use auth::run_auth_status;
pub(crate) use commands::{run_config, run_init};
pub(crate) use interactive::run_config_interactive;
pub(crate) use reset::run_reset;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub type ConfigError = Box<dyn std::error::Error>;
/// Let Codex use its configured model unless the user explicitly overrides it.
pub const DEFAULT_CODEX_MODEL: &str = "default";
pub const DEFAULT_CODEX_REASONING_EFFORT: &str = "xhigh";

/// Supported languages for LLM output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    Ko,
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lang::En => write!(f, "en"),
            Lang::Ko => write!(f, "ko"),
        }
    }
}

/// Top-level config file structure.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
    pub output: OutputConfig,
    /// Sensitive data masking config. None means default-enabled.
    pub redactor: Option<RedactorConfig>,
    /// LLM output language. None triggers migration prompt on first use.
    pub lang: Option<Lang>,
    /// Optional input root overrides for agent session logs.
    #[serde(default)]
    pub input: Option<InputConfig>,
}

impl Config {
    /// Returns whether the redactor is enabled (defaults to true when absent).
    pub fn is_redactor_enabled(&self) -> bool {
        self.redactor.as_ref().is_none_or(|r| r.enabled)
    }
}

/// LLM provider settings.
#[derive(Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub provider: String,
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default)]
    pub anthropic_api_key: String,
    /// Legacy single API key field (`api_key`) kept only for migration.
    #[serde(default, rename = "api_key", skip_serializing)]
    pub legacy_api_key: String,
    #[serde(default)]
    pub codex_model: Option<String>,
    #[serde(default)]
    pub codex_reasoning_effort: Option<String>,
}

impl LlmConfig {
    /// Returns API key for the given provider.
    fn api_key_for_provider(&self, provider: &str) -> &str {
        match provider {
            "openai" => &self.openai_api_key,
            "anthropic" => &self.anthropic_api_key,
            _ => "",
        }
    }

    /// Sets API key for the given provider.
    fn set_api_key_for_provider(&mut self, provider: &str, api_key: String) {
        match provider {
            "openai" => self.openai_api_key = api_key,
            "anthropic" => self.anthropic_api_key = api_key,
            _ => {}
        }
    }

    /// Migrates legacy `api_key` into provider-specific key fields.
    /// Returns true when migration changed in-memory values.
    fn migrate_legacy_api_key(&mut self) -> bool {
        let legacy = self.legacy_api_key.trim();
        if legacy.is_empty() {
            return false;
        }

        let mut changed = false;

        // Infer provider from known key prefix first.
        if legacy.starts_with("sk-ant-") {
            if self.anthropic_api_key.is_empty() {
                self.anthropic_api_key = legacy.to_string();
                changed = true;
            }
        } else if legacy.starts_with("sk-") {
            if self.openai_api_key.is_empty() {
                self.openai_api_key = legacy.to_string();
                changed = true;
            }
        } else {
            match self.provider.as_str() {
                "openai" => {
                    if self.openai_api_key.is_empty() {
                        self.openai_api_key = legacy.to_string();
                        changed = true;
                    }
                }
                "anthropic" => {
                    if self.anthropic_api_key.is_empty() {
                        self.anthropic_api_key = legacy.to_string();
                        changed = true;
                    }
                }
                _ => {
                    if self.openai_api_key.is_empty() {
                        self.openai_api_key = legacy.to_string();
                        changed = true;
                    }
                }
            }
        }

        if changed {
            self.legacy_api_key.clear();
        }

        changed
    }
}

/// Markdown output settings.
/// `path` stores the vault root; `save_to_vault()` appends sub-directories.
#[derive(Debug, Serialize, Deserialize)]
pub struct OutputConfig {
    pub path: String,
}

/// Redactor (sensitive data masking) settings.
#[derive(Debug, Serialize, Deserialize)]
pub struct RedactorConfig {
    pub enabled: bool,
}

/// Optional input overrides for agent log discovery roots.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InputConfig {
    #[serde(default)]
    pub codex_roots: Option<Vec<String>>,
    #[serde(default)]
    pub claude_roots: Option<Vec<String>>,
    #[serde(default)]
    pub claude: Option<ClaudeInputConfig>,
    #[serde(default)]
    pub antigravity: Option<AntigravityInputConfig>,
}

/// Claude-specific input options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeInputConfig {
    #[serde(default)]
    pub roots: Option<Vec<String>>,
    #[serde(default)]
    pub include_automated: bool,
}

/// Antigravity-specific input options (`[input.antigravity]`).
/// A missing section means the defaults: enabled, no extra roots, `agy -p` runs skipped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityInputConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Extra `brain` folders searched before the default ones.
    #[serde(default)]
    pub roots: Option<Vec<String>>,
    /// Also analyze non-interactive `agy -p` runs.
    #[serde(default)]
    pub include_print_mode: bool,
}

impl Default for AntigravityInputConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            roots: None,
            include_print_mode: false,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Returns the config file path: ~/.config/rwd/config.toml
pub fn config_path() -> Result<PathBuf, ConfigError> {
    let home = dirs::home_dir().ok_or(crate::messages::error::HOME_DIR_NOT_FOUND)?;
    Ok(home.join(".config").join("rwd").join("config.toml"))
}

/// Saves config to a TOML file. Creates parent directories if needed.
pub fn save_config(config: &Config, path: &std::path::Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml_str = toml::to_string_pretty(config)?;
    std::fs::write(path, toml_str)?;

    // Restrict to owner-only read/write (0o600) since the file contains an API key.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Loads config from a TOML file.
pub fn load_config(path: &std::path::Path) -> Result<Config, ConfigError> {
    let content = std::fs::read_to_string(path)?;
    let mut config: Config = toml::from_str(&content)?;
    if config.llm.migrate_legacy_api_key() {
        // Best-effort in-place migration to persist new key layout and drop legacy field.
        let _ = save_config(&config, path);
    }
    Ok(config)
}

/// Loads config if the file exists; returns None otherwise.
pub fn load_config_if_exists() -> Option<Config> {
    let path = config_path().ok()?;
    if path.exists() {
        load_config(&path).ok()
    } else {
        None
    }
}

/// Resolves `${VAR}` placeholders in `template` from process environment.
///
/// Used to keep API keys out of config.toml on disk: users may write
/// `openai_api_key = "${OPENAI_API_KEY}"` and have rwd read the actual
/// secret from the shell environment at runtime. Only `${NAME}` is
/// recognized (no `$NAME` shorthand) and only ASCII letters, digits, and
/// underscores are allowed in the variable name. Strings with no placeholder
/// pass through unchanged so existing literal-key configs keep working.
pub fn resolve_env_placeholders(template: &str) -> Result<String, ConfigError> {
    if !template.contains("${") {
        return Ok(template.to_string());
    }

    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i + 2;
            let Some(end_off) = bytes[start..].iter().position(|&b| b == b'}') else {
                out.push_str(&template[i..]);
                break;
            };
            let end = start + end_off;
            let name = &template[start..end];
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                // Not a valid env-var reference; emit literally.
                out.push_str(&template[i..=end]);
                i = end + 1;
                continue;
            }
            let value =
                std::env::var(name).map_err(|_| crate::messages::config::missing_env_var(name))?;
            out.push_str(&value);
            i = end + 1;
        } else {
            // Push one UTF-8 char at a time to avoid splitting code points.
            let ch_len = template[i..]
                .chars()
                .next()
                .map(|c| c.len_utf8())
                .unwrap_or(1);
            out.push_str(&template[i..i + ch_len]);
            i += ch_len;
        }
    }
    Ok(out)
}

/// Parses codex model config value.
/// "default" (case-insensitive) resets to built-in default.
fn parse_codex_model_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_bool_toggle(value: &str, key_name: &str) -> Result<bool, ConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("Invalid value for `{key_name}`: '{value}'. Use true/false.").into()),
    }
}

/// Parses codex reasoning effort config value.
/// Returns None when value is "default".
fn parse_reasoning_effort(value: &str) -> Result<Option<String>, ConfigError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("default") {
        return Ok(None);
    }

    let normalized = trimmed.to_ascii_lowercase();
    let allowed = ["low", "medium", "high", "xhigh"];
    if allowed.contains(&normalized.as_str()) {
        Ok(Some(normalized))
    } else {
        Err(crate::messages::config::unsupported_reasoning_effort(trimmed).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path_includes_rwd_dir() {
        let path = config_path().expect("path creation");
        assert!(path.ends_with("rwd/config.toml"));
    }

    #[test]
    fn test_save_and_load_config_roundtrip() {
        let temp_dir = std::env::temp_dir().join("rwd_test_config");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("create dir");
        let path = temp_dir.join("config.toml");

        let config = Config {
            llm: LlmConfig {
                provider: "anthropic".to_string(),
                openai_api_key: String::new(),
                anthropic_api_key: "sk-test-key".to_string(),
                legacy_api_key: String::new(),
                codex_model: None,
                codex_reasoning_effort: None,
            },
            output: OutputConfig {
                path: "/tmp/vault".to_string(),
            },
            redactor: None,
            lang: Some(Lang::En),
            input: None,
        };

        save_config(&config, &path).expect("save");
        let loaded = load_config(&path).expect("load");

        assert_eq!(loaded.llm.provider, "anthropic");
        assert_eq!(loaded.llm.openai_api_key, "");
        assert_eq!(loaded.llm.anthropic_api_key, "sk-test-key");
        assert_eq!(loaded.llm.codex_model, None);
        assert_eq!(loaded.llm.codex_reasoning_effort, None);
        assert_eq!(loaded.output.path, "/tmp/vault");
        assert_eq!(loaded.lang, Some(Lang::En));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_config_redactor_none_when_missing() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        assert!(config.redactor.is_none());
    }

    #[test]
    fn test_config_redactor_parses_when_present() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"

[redactor]
enabled = false
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        assert!(!config.redactor.unwrap().enabled);
    }

    #[test]
    fn test_config_lang_none_when_missing() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        assert!(config.lang.is_none());
    }

    #[test]
    fn test_config_lang_parses_ko() {
        // lang must appear before table sections in TOML
        let toml_str = r#"
lang = "ko"

[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        assert_eq!(config.lang, Some(Lang::Ko));
    }

    #[test]
    fn test_config_lang_parses_en() {
        let toml_str = r#"
lang = "en"

[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        assert_eq!(config.lang, Some(Lang::En));
    }

    #[test]
    fn test_config_input_none_when_missing() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        assert!(config.input.is_none());
    }

    #[test]
    fn test_config_input_parses_root_overrides() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"

[input]
codex_roots = ["/home/example/.codex/sessions", "/mnt/c/Users/example/.codex/sessions"]
claude_roots = ["/home/example/.claude/projects"]
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        let input = config.input.expect("input section");
        assert_eq!(
            input.codex_roots,
            Some(vec![
                "/home/example/.codex/sessions".to_string(),
                "/mnt/c/Users/example/.codex/sessions".to_string()
            ])
        );
        assert_eq!(
            input.claude_roots,
            Some(vec!["/home/example/.claude/projects".to_string()])
        );
        assert!(input.claude.is_none());
    }

    #[test]
    fn test_config_input_parses_nested_claude_options() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"

[input.claude]
roots = ["/mnt/c/Users/example/.claude/projects"]
include_automated = true
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        let input = config.input.expect("input section");
        let claude = input.claude.expect("claude section");
        assert_eq!(
            claude.roots,
            Some(vec!["/mnt/c/Users/example/.claude/projects".to_string()])
        );
        assert!(claude.include_automated);
    }

    #[test]
    fn test_config_input_antigravity_none_when_section_missing() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"

[input.claude]
include_automated = true
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        let input = config.input.expect("input section");
        assert!(input.antigravity.is_none());
        let defaults = AntigravityInputConfig::default();
        assert!(defaults.enabled);
        assert!(defaults.roots.is_none());
        assert!(!defaults.include_print_mode);
    }

    #[test]
    fn test_config_input_antigravity_partial_section_uses_defaults() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"

[input.antigravity]
roots = ["/mnt/c/Users/example/.gemini/antigravity/brain"]
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        let antigravity = config
            .input
            .and_then(|input| input.antigravity)
            .expect("antigravity section");
        assert!(antigravity.enabled);
        assert!(!antigravity.include_print_mode);
        assert_eq!(
            antigravity.roots,
            Some(vec![
                "/mnt/c/Users/example/.gemini/antigravity/brain".to_string()
            ])
        );
    }

    #[test]
    fn test_config_input_antigravity_parses_disabled_and_print_mode() {
        let toml_str = r#"
[llm]
provider = "anthropic"
anthropic_api_key = "sk-test"

[output]
path = "/tmp/vault"

[input.antigravity]
enabled = false
include_print_mode = true
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        let antigravity = config
            .input
            .and_then(|input| input.antigravity)
            .expect("antigravity section");
        assert!(!antigravity.enabled);
        assert!(antigravity.include_print_mode);
    }

    #[test]
    fn test_parse_bool_toggle_accepts_true_false_values() {
        assert!(parse_bool_toggle("true", "k").expect("true"));
        assert!(parse_bool_toggle("1", "k").expect("1"));
        assert!(!parse_bool_toggle("false", "k").expect("false"));
        assert!(!parse_bool_toggle("off", "k").expect("off"));
    }

    #[test]
    fn test_lang_display() {
        assert_eq!(Lang::En.to_string(), "en");
        assert_eq!(Lang::Ko.to_string(), "ko");
    }

    #[test]
    fn test_lang_roundtrip_serialization() {
        let config = Config {
            llm: LlmConfig {
                provider: "anthropic".to_string(),
                openai_api_key: String::new(),
                anthropic_api_key: "sk-test".to_string(),
                legacy_api_key: String::new(),
                codex_model: None,
                codex_reasoning_effort: None,
            },
            output: OutputConfig {
                path: "/tmp/vault".to_string(),
            },
            redactor: None,
            lang: Some(Lang::Ko),
            input: None,
        };
        let serialized = toml::to_string_pretty(&config).expect("serialize");
        assert!(serialized.contains("lang = \"ko\""));
        let loaded: Config = toml::from_str(&serialized).expect("deserialize");
        assert_eq!(loaded.lang, Some(Lang::Ko));
    }

    #[test]
    fn test_parse_codex_model_default_returns_none() {
        assert_eq!(parse_codex_model_value("default"), None);
        assert_eq!(parse_codex_model_value(""), None);
        assert_eq!(
            parse_codex_model_value("gpt-5.4"),
            Some("gpt-5.4".to_string())
        );
    }

    #[test]
    fn test_parse_reasoning_effort_valid_values() {
        assert_eq!(
            parse_reasoning_effort("xhigh").expect("parse"),
            Some("xhigh".to_string())
        );
        assert_eq!(
            parse_reasoning_effort("HIGH").expect("parse"),
            Some("high".to_string())
        );
    }

    #[test]
    fn test_parse_reasoning_effort_default_returns_none() {
        assert_eq!(parse_reasoning_effort("default").expect("parse"), None);
        assert_eq!(parse_reasoning_effort("").expect("parse"), None);
    }

    #[test]
    fn test_parse_reasoning_effort_invalid_returns_error() {
        let result = parse_reasoning_effort("turbo");
        assert!(result.is_err());
    }

    #[test]
    fn test_config_llm_codex_fields_parse_when_present() {
        let toml_str = r#"
[llm]
provider = "codex"
openai_api_key = ""
anthropic_api_key = ""
codex_model = "gpt-5.4"
codex_reasoning_effort = "xhigh"

[output]
path = "/tmp/vault"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse");
        assert_eq!(config.llm.provider, "codex");
        assert_eq!(config.llm.codex_model.as_deref(), Some("gpt-5.4"));
        assert_eq!(config.llm.codex_reasoning_effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn test_load_config_migrates_legacy_openai_api_key() {
        let temp_dir = std::env::temp_dir().join("rwd_test_legacy_key_openai");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("create dir");
        let path = temp_dir.join("config.toml");

        let legacy_toml = r#"
[llm]
provider = "openai"
api_key = "sk-openai-legacy"

[output]
path = "/tmp/vault"
"#;
        std::fs::write(&path, legacy_toml).expect("write legacy config");

        let loaded = load_config(&path).expect("load config");
        assert_eq!(loaded.llm.openai_api_key, "sk-openai-legacy");
        assert_eq!(loaded.llm.anthropic_api_key, "");

        let migrated = std::fs::read_to_string(&path).expect("read migrated config");
        assert!(migrated.contains("openai_api_key = \"sk-openai-legacy\""));
        assert!(!migrated.contains("\napi_key ="));

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_load_config_migrates_legacy_anthropic_key_by_prefix() {
        let temp_dir = std::env::temp_dir().join("rwd_test_legacy_key_anthropic");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("create dir");
        let path = temp_dir.join("config.toml");

        let legacy_toml = r#"
[llm]
provider = "codex"
api_key = "sk-ant-legacy"

[output]
path = "/tmp/vault"
"#;
        std::fs::write(&path, legacy_toml).expect("write legacy config");

        let loaded = load_config(&path).expect("load config");
        assert_eq!(loaded.llm.openai_api_key, "");
        assert_eq!(loaded.llm.anthropic_api_key, "sk-ant-legacy");

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_resolve_env_placeholders_passthrough_when_no_placeholder() {
        assert_eq!(
            resolve_env_placeholders("literal-value").expect("literal"),
            "literal-value"
        );
    }

    #[test]
    fn test_resolve_env_placeholders_substitutes_known_var() {
        let path = std::env::var("PATH").expect("test process PATH");
        let resolved = resolve_env_placeholders("한글-${PATH}-${PATH}").expect("resolved");
        assert_eq!(resolved, format!("한글-{path}-{path}"));
    }

    #[test]
    fn test_resolve_env_placeholders_errors_on_missing_var() {
        let name = format!("RWD_TEST_UNSET_{}", std::process::id());
        assert!(std::env::var_os(&name).is_none());
        let error = resolve_env_placeholders(&format!("${{{name}}}")).unwrap_err();
        assert!(error.to_string().contains(&name));
    }

    #[test]
    fn test_resolve_env_placeholders_emits_invalid_token_literally() {
        for value in ["hello ${not a var} world", "${}"] {
            assert_eq!(resolve_env_placeholders(value).expect("literal"), value);
        }
    }

    #[test]
    fn test_resolve_env_placeholders_unterminated_brace_passes_through() {
        assert_eq!(
            resolve_env_placeholders("${OPEN_AI_KEY").expect("literal"),
            "${OPEN_AI_KEY"
        );
    }
}
