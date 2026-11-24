// Converts analysis results to Markdown and saves them to the Obsidian vault.

mod managed;
pub mod markdown;
pub(crate) mod retrospective;
mod vault;

// TODO: Replace with a dedicated error type via thiserror.
pub type OutputError = Box<dyn std::error::Error>;

#[cfg(test)]
pub use markdown::render_combined_markdown;
pub use markdown::render_daily_note;

use std::path::{Path, PathBuf};

use crate::analyzer::insight::Evidence;
use chrono::NaiveDate;

/// Loads the Obsidian vault path from config (~/.config/rwd/config.toml).
pub fn load_vault_path() -> Result<PathBuf, OutputError> {
    let config = crate::config::load_required_config()?;

    let path = crate::config::expand_tilde(&config.output.path);
    if !path.exists() {
        std::fs::create_dir_all(&path)?;
    }
    Ok(path)
}

/// Updates only the unchanged rwd analysis region in the dated note.
/// The output.path is expected to be the final directory (e.g., vault/Daily),
/// so no additional subdirectories are appended.
pub fn save_to_vault(
    vault_path: &Path,
    date: NaiveDate,
    content: &str,
) -> Result<PathBuf, OutputError> {
    vault::save(vault_path, date, managed::Section::Analysis, content)
}

/// Full directory keys keep equal leaf names separate. Unknown remains explicit.
pub(crate) fn project_groups<'a>(
    sources: &[(&'a str, &'a crate::analyzer::AnalysisResult)],
) -> std::collections::BTreeMap<
    Option<crate::analyzer::retrospective::Project>,
    Vec<(&'a str, &'a crate::analyzer::insight::SessionInsight)>,
> {
    let mut groups = std::collections::BTreeMap::new();
    for (name, analysis) in sources {
        for session in &analysis.sessions {
            let project = session.project.as_ref().and_then(|p| {
                crate::analyzer::retrospective::Project::from_directories([p.directory.as_str()])
            });
            groups
                .entry(project)
                .or_insert_with(Vec::new)
                .push((*name, session));
        }
    }
    groups
}

pub(crate) fn project_label(project: Option<&crate::analyzer::retrospective::Project>) -> String {
    project.map_or_else(|| "Project unknown / 프로젝트 미확인".into(), |p| p.label())
}

/// Shared evidence text. Strip control characters before printing to a terminal.
pub fn display_evidence(evidence: &[Evidence]) -> String {
    let mut output = String::new();
    for proof in evidence {
        if let Some(location) = &proof.location {
            let time = proof
                .timestamp
                .map(|ts| {
                    format!(
                        " {} {}",
                        ts.to_rfc3339(),
                        if proof.is_reference {
                            "Reference / 이전 참고"
                        } else {
                            "Target day / 대상 날짜"
                        }
                    )
                })
                .unwrap_or_default();
            output.push_str(&format!(
                "    Evidence / 근거: {} {}:{} [{}]{}\n    Quote / 발언: {}\n",
                plain(&proof.role),
                plain(&location.path.display().to_string()),
                location.line,
                plain(&proof.message_id),
                time,
                plain(&proof.quote)
            ));
        }
    }
    if output.is_empty() {
        output.push_str(&format!("    {EVIDENCE_UNAVAILABLE}\n"));
    }
    output
}

pub(crate) const EVIDENCE_UNAVAILABLE: &str = "Evidence unavailable / 근거 미확인 (기존 결과 포함)";

/// Compact rows for quotations that still resolve to a log line. The path is
/// left out when it is the session log already named at the top of the block.
pub(crate) fn evidence_items(evidence: &[Evidence], log: Option<&Path>) -> Vec<String> {
    let mut items = Vec::new();
    for proof in evidence {
        let Some(location) = &proof.location else {
            continue;
        };
        let place = if log == Some(location.path.as_path()) {
            format!("line {}", location.line)
        } else {
            format!("{}:{}", location.path.display(), location.line)
        };
        let time = proof
            .timestamp
            .map(|ts| format!(" {}", ts.format("%Y-%m-%dT%H:%M:%SZ")))
            .unwrap_or_default();
        let reference = if proof.is_reference {
            " (Reference / 이전 참고)"
        } else {
            ""
        };
        items.push(plain(&format!(
            "- {} {place} [{}]{time}{reference}: {}",
            proof.role, proof.message_id, proof.quote
        )));
    }
    items
}

/// More TIL than this per session buries the ones worth rereading.
pub(crate) const MAX_TIL_PER_SESSION: usize = 3;

pub(crate) fn shown_til(
    session: &crate::analyzer::insight::SessionInsight,
) -> impl Iterator<Item = &crate::analyzer::insight::TilItem> {
    session.til.iter().take(MAX_TIL_PER_SESSION)
}

/// A session with only a summary is listed in one line instead of a block.
pub(crate) fn is_short_session(session: &crate::analyzer::insight::SessionInsight) -> bool {
    session.decisions.is_empty()
        && session.corrections.is_empty()
        && session.til.is_empty()
        && retrospective::unresolved_questions(session)
            .next()
            .is_none()
}

pub(crate) fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

pub(crate) fn escape_markdown(text: &str) -> String {
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
            timestamp: None,
            is_reference: false,
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
    fn test_save_preserves_existing_notes_on_reanalysis() {
        let dir = std::env::temp_dir().join(format!("rwd_preserve_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let date = NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        let path = dir.join(format!("{date}.md"));
        let note =
            "---\ntitle: My day\n---\n# My day\n\n## Development Progress\nManual progress\n";
        std::fs::write(&path, note).unwrap();
        save_to_vault(&dir, date, "New analysis").unwrap();
        save_to_vault(&dir, date, "Reanalysis").unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(saved.starts_with(note));
        assert!(saved.contains("Reanalysis"));
        assert!(!saved.contains("New analysis"));
    }

    /// Writes the region the removed `rwd summary` command left in older notes.
    fn save_legacy_summary(dir: &Path, date: NaiveDate, summary: &str) {
        let body = format!("## Development Progress\n\n{summary}\n");
        vault::save(dir, date, managed::Section::LegacySummary, &body).unwrap();
    }

    #[test]
    fn test_legacy_summary_region_and_user_content_survive_analysis_saves() {
        let date = NaiveDate::from_ymd_opt(2099, 1, 2).unwrap();
        let analysis = crate::analyzer::AnalysisResult { sessions: vec![] };
        let markdown = render_combined_markdown(&[("Synthetic", &analysis)], date);
        for summary_first in [true, false] {
            let dir = std::env::temp_dir()
                .join(format!("rwd_order_{}_{summary_first}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join(format!("{date}.md"));
            let note = "---\ntitle: Mine\n---\n# My title\n\n## Development Progress\nHuman summary\n\n```md\n## Development Progress\nExample\n```\n";
            std::fs::write(&path, note).unwrap();
            if summary_first {
                save_legacy_summary(&dir, date, "latest progress");
            }
            save_to_vault(&dir, date, &markdown).unwrap();
            if !summary_first {
                save_legacy_summary(&dir, date, "latest progress");
            }
            let with_memo = format!(
                "{}\n## My memo\nKeep this\n",
                std::fs::read_to_string(&path).unwrap()
            );
            std::fs::write(&path, &with_memo).unwrap();
            // Cached rendering and a refreshed analysis use the identical save boundary.
            save_to_vault(&dir, date, &markdown).unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), with_memo);
            save_to_vault(&dir, date, "refreshed analysis").unwrap();
            let saved = std::fs::read_to_string(&path).unwrap();
            assert!(saved.starts_with(note));
            assert!(saved.ends_with("\n## My memo\nKeep this\n"));
            assert_eq!(saved.matches("latest progress").count(), 1);
            assert_eq!(saved.matches("refreshed analysis").count(), 1);
            assert_eq!(saved.matches("<!-- rwd:analysis:v1:").count(), 1);
            assert_eq!(saved.matches("<!-- rwd:summary:v1:").count(), 1);
            // Editing either managed region must stop the writer without mutation.
            for text in ["latest progress", "refreshed analysis"] {
                let manual = saved.replace(text, "human addition");
                std::fs::write(&path, &manual).unwrap();
                assert!(save_to_vault(&dir, date, "new").is_err());
                assert_eq!(std::fs::read_to_string(&path).unwrap(), manual);
            }
            std::fs::remove_dir_all(dir).unwrap();
        }
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
        let saved = std::fs::read_to_string(&saved_path).expect("read file");
        assert!(saved.contains(content));
        assert_eq!(save_to_vault(&temp_dir, date, content).unwrap(), saved_path);
        assert_eq!(std::fs::read_to_string(&saved_path).unwrap(), saved);

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}
