//! Project identity and conversation-backed retrospective state.

use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// A historical working directory, not an inferred Git repository identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Project {
    pub directory: String,
}

impl Project {
    pub fn from_directories<'a>(directories: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        let mut unique = std::collections::BTreeSet::new();
        for directory in directories {
            // Never display credential URLs or feed masked identities into grouping.
            if directory.contains("://")
                || directory.chars().any(char::is_control)
                || crate::redactor::redact_text(directory).0 != directory
            {
                return None;
            }
            let path = Path::new(directory);
            if !path.is_absolute() {
                return None;
            }
            let mut normalized = PathBuf::new();
            for part in path.components() {
                match part {
                    Component::ParentDir => {
                        if !normalized.pop() {
                            return None;
                        }
                    }
                    Component::CurDir => {}
                    part => normalized.push(part.as_os_str()),
                }
            }
            normalized.file_name()?;
            unique.insert(normalized.to_str()?.to_string());
        }
        if unique.len() != 1 {
            return None;
        }
        Some(Self {
            directory: unique.into_iter().next()?,
        })
    }

    pub fn name(&self) -> &str {
        Path::new(&self.directory)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("Project unknown / 프로젝트 미확인")
    }

    pub fn label(&self) -> String {
        format!("{} ({})", self.name(), self.directory)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builders_use_only_explicit_project_metadata() {
        let entries = vec![
            serde_json::from_value(serde_json::json!({
                "type":"user","sessionId":"s1","uuid":"u1","timestamp":"2099-01-01T12:00:00Z",
                "cwd":"/workspace/app","message":{"content":"work on another repo"}
            }))
            .unwrap(),
        ];
        let tasks = crate::analyzer::build_claude_session_tasks(
            &entries,
            "claude",
            &std::collections::HashMap::new(),
        )
        .unwrap();
        assert_eq!(
            tasks[0].project.as_ref().unwrap().directory,
            "/workspace/app"
        );
        let codex = crate::parser::codex::CodexEntry::from_raw(serde_json::from_str(r#"{"timestamp":"2099-01-01T12:00:00Z","type":"session_meta","payload":{"id":"s1","cwd":"/other/app"}}"#).unwrap());
        let speech = crate::parser::codex::CodexEntry::from_raw(serde_json::from_str(r#"{"timestamp":"2099-01-01T12:01:00Z","type":"event_msg","payload":{"type":"user_message","message":"help"}}"#).unwrap());
        let task = crate::analyzer::build_codex_session_task(&[codex, speech], "s1", "codex", &[])
            .unwrap();
        assert_eq!(task.project.as_ref().unwrap().directory, "/other/app");
    }

    #[test]
    fn test_project_uses_full_local_path_and_never_merges_same_leaf() {
        let a = Project::from_directories(["/a/app/./src/.."]);
        let b = Project::from_directories(["/b/app"]);
        assert_eq!(a.as_ref().unwrap().directory, "/a/app");
        assert_ne!(a, b);
        assert_eq!(a.unwrap().name(), "app");
        assert!(Project::from_directories(["/a/app", "/b/app"]).is_none());
        assert_eq!(
            Project::from_directories(["/a//app/", "/a/app"]),
            Project::from_directories(["/a/app"])
        );
    }

    #[test]
    fn test_project_missing_relative_url_and_secret_are_unknown() {
        for value in [
            "",
            "app",
            "https://name:password@host/app",
            "/https://name:password@host/app",
            "/a/../..",
            "/",
        ] {
            assert!(Project::from_directories([value]).is_none(), "{value}");
        }
        let secret = format!("/tmp/sk-proj-{}/app", "a".repeat(40));
        assert!(Project::from_directories([secret.as_str()]).is_none());
        assert!(Project::from_directories(["/valid", secret.as_str()]).is_none());
    }
}
