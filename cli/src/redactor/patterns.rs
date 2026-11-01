// Built-in sensitive data detection patterns.
// Each regex is compiled once via LazyLock and reused.

use regex::Regex;
use std::sync::LazyLock;

/// Pattern kind — metadata only, both currently use Regex.
#[allow(dead_code)]
pub enum PatternKind {
    FixedPrefix,
    Regex,
}

/// A single masking rule.
pub struct RedactorRule {
    pub name: &'static str,
    #[allow(dead_code)]
    pub kind: PatternKind,
    pub pattern: &'static Regex,
}

/// Returns the list of built-in redaction rules.
pub fn builtin_rules() -> Vec<RedactorRule> {
    static API_KEY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"sk-[a-zA-Z0-9_-]{20,}").expect("API_KEY regex"));
    static AWS_KEY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"AKIA[0-9A-Z]{16}").expect("AWS_KEY regex"));
    static GITHUB_TOKEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"gh[ps]_[a-zA-Z0-9]{36,}").expect("GITHUB_TOKEN regex"));
    static SLACK_TOKEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"xox[bpsa]-[a-zA-Z0-9\-]+").expect("SLACK_TOKEN regex"));
    static BEARER_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"Bearer\s+[a-zA-Z0-9\-._~+/]+=*").expect("BEARER_TOKEN regex")
    });
    static ENV_SECRET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?i)(password|secret|api_key)\s*=\s*["'][^"']+["']"#)
            .expect("ENV_SECRET regex")
    });
    static PRIVATE_IP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(10\.\d+\.\d+\.\d+|172\.(1[6-9]|2\d|3[01])\.\d+\.\d+|192\.168\.\d+\.\d+)")
            .expect("PRIVATE_IP regex")
    });
    static PRIVATE_KEY: LazyLock<Regex> = LazyLock::new(|| {
        // An incomplete block consumes the remaining input rather than exposing
        // a body whose END marker was omitted by the source log.
        Regex::new(
            r"(?s)-----BEGIN[A-Z ]*PRIVATE KEY-----.*?(?:-----END[A-Z ]*PRIVATE KEY-----|\z)",
        )
        .expect("PRIVATE_KEY regex")
    });

    vec![
        RedactorRule {
            name: "PRIVATE_KEY",
            kind: PatternKind::Regex,
            pattern: &PRIVATE_KEY,
        },
        RedactorRule {
            name: "ENV_SECRET",
            kind: PatternKind::Regex,
            pattern: &ENV_SECRET,
        },
        RedactorRule {
            name: "BEARER_TOKEN",
            kind: PatternKind::Regex,
            pattern: &BEARER_TOKEN,
        },
        RedactorRule {
            name: "API_KEY",
            kind: PatternKind::FixedPrefix,
            pattern: &API_KEY,
        },
        RedactorRule {
            name: "AWS_KEY",
            kind: PatternKind::FixedPrefix,
            pattern: &AWS_KEY,
        },
        RedactorRule {
            name: "GITHUB_TOKEN",
            kind: PatternKind::FixedPrefix,
            pattern: &GITHUB_TOKEN,
        },
        RedactorRule {
            name: "SLACK_TOKEN",
            kind: PatternKind::FixedPrefix,
            pattern: &SLACK_TOKEN,
        },
        RedactorRule {
            name: "PRIVATE_IP",
            kind: PatternKind::Regex,
            pattern: &PRIVATE_IP,
        },
    ]
}
