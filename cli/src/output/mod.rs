// Converts analysis results to Markdown and saves them to the Obsidian vault.

pub mod markdown;

// TODO: Replace with a dedicated error type via thiserror.
pub type OutputError = Box<dyn std::error::Error>;

pub use markdown::render_combined_markdown;

use std::path::{Path, PathBuf};

use crate::analyzer::insight::Evidence;
use chrono::NaiveDate;

/// Loads the Obsidian vault path from config (~/.config/rwd/config.toml).
pub fn load_vault_path() -> Result<PathBuf, OutputError> {
    let config = crate::config::load_config_if_exists().ok_or(crate::messages::error::NO_CONFIG)?;

    let path = PathBuf::from(&config.output.path);
    if !path.exists() {
        std::fs::create_dir_all(&path)?;
    }
    Ok(path)
}

/// Saves Markdown content to the output path with a date-based filename.
/// The output.path is expected to be the final directory (e.g., vault/Daily),
/// so no additional subdirectories are appended.
pub fn save_to_vault(
    vault_path: &Path,
    date: NaiveDate,
    content: &str,
) -> Result<PathBuf, OutputError> {
    std::fs::create_dir_all(vault_path)?;

    let filename = format!("{date}.md");
    let file_path = vault_path.join(&filename);

    std::fs::write(&file_path, content)?;

    Ok(file_path)
}

/// Shared evidence text. Strip control characters before printing to a terminal.
pub fn display_evidence(evidence: &[Evidence]) -> String {
    let mut output = String::new();
    for proof in evidence {
        if let Some(location) = &proof.location {
            output.push_str(&format!(
                "    Evidence / 근거: {} {}:{} [{}]\n    Quote / 발언: {}\n",
                plain(&proof.role),
                plain(&location.path.display().to_string()),
                location.line,
                plain(&proof.message_id),
                plain(&proof.quote)
            ));
        }
    }
    if output.is_empty() {
        output.push_str("    Evidence unavailable / 근거 미확인 (기존 결과 포함)\n");
    }
    output
}

fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn escape_markdown(text: &str) -> String {
    let mut output = String::new();
    for c in text.chars() {
        match c {
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '&' => output.push_str("&amp;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '!' | '#' | '|' => {
                output.push('\\');
                output.push(c);
            }
            c if c.is_control() => output.push(' '),
            _ => output.push(c),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evidence_display_cannot_inject_markdown_or_terminal_controls() {
        let proof = Evidence {
            message_id: "m1".into(),
            role: "USER".into(),
            quote: "<script>\n[click](https://invalid.test)\u{1b}[31m".into(),
            location: Some(crate::parser::SourceLocation::new(
                Path::new("/tmp/a.jsonl"),
                1,
            )),
        };
        let rendered = display_evidence(&[proof]);
        assert!(!rendered.contains('\u{1b}'));
        assert!(rendered.contains("[click](https://invalid.test)"));
        assert_eq!(rendered.lines().count(), 2);
        let markdown = escape_markdown(&rendered);
        assert!(!markdown.contains('<'));
        assert!(markdown.contains("\\[click\\]\\(https://invalid.test\\)"));
    }

    #[test]
    fn test_save_to_vault_creates_file() {
        let temp_dir = std::env::temp_dir().join("rwd_test_output");
        std::fs::create_dir_all(&temp_dir).expect("failed to create temp dir");

        let date = NaiveDate::from_ymd_opt(2026, 3, 11).expect("valid date");
        let content = "# 테스트 Markdown";

        let result = save_to_vault(&temp_dir, date, content);
        assert!(result.is_ok());

        let saved_path = result.expect("save succeeded");
        assert!(saved_path.exists());
        assert!(saved_path.starts_with(&temp_dir));
        assert_eq!(
            std::fs::read_to_string(&saved_path).expect("read file"),
            content
        );

        std::fs::remove_file(&saved_path).ok();
        std::fs::remove_dir(&temp_dir).ok();
    }
}
