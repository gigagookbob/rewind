//! Project identity and conversation-backed retrospective state.

use super::insight::{Evidence, string_or_stringify, vec_or_default};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskTitle {
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub text: String,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Question {
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub question: String,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub evidence: Vec<Evidence>,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub updates: Vec<QuestionUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionUpdate {
    #[serde(default)]
    pub kind: QuestionUpdateKind,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub detail: String,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionUpdateKind {
    Answer,
    Resolved,
    Open,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUp {
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub task: String,
    #[serde(default)]
    pub origin: FollowUpOrigin,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub evidence: Vec<Evidence>,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub updates: Vec<FollowUpUpdate>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpOrigin {
    UserRequest,
    AiSuggestion,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FollowUpUpdate {
    #[serde(default)]
    pub kind: FollowUpUpdateKind,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub detail: String,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpUpdateKind {
    Accepted,
    Completed,
    Withdrawn,
    #[default]
    #[serde(other)]
    Unknown,
}

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
            let directory = claude_worktree_repository(&normalized).unwrap_or(normalized);
            directory.file_name()?;
            unique.insert(directory.to_str()?.to_string());
        }
        // A session that moves into subdirectories still belongs to the recorded
        // parent. Unrelated or sibling-only directories stay unknown.
        let root = unique
            .iter()
            .min_by_key(|directory| Path::new(directory).components().count())?;
        if !unique
            .iter()
            .all(|directory| Path::new(directory).starts_with(root))
        {
            return None;
        }
        Some(Self {
            directory: root.clone(),
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

/// Returns `<repository>` for `<repository>/.claude/worktrees/<name>` and below.
///
/// Claude Code creates its worktrees inside the repository, so the recorded
/// path alone names the repository. Only the path shape is checked: the
/// worktree is usually deleted before the log is analyzed. Worktrees created
/// elsewhere do not record their repository and are left as they are.
fn claude_worktree_repository(path: &Path) -> Option<PathBuf> {
    let parts: Vec<Component> = path.components().collect();
    // `windows(3)` also requires a worktree name after the two marker folders.
    let marker = parts.windows(3).position(|window| {
        window[0].as_os_str() == ".claude" && window[1].as_os_str() == "worktrees"
    })?;
    let repository: PathBuf = parts[..marker].iter().collect();
    // The marker directly under the filesystem root has no repository folder.
    repository.file_name()?;
    Some(repository)
}

impl Question {
    /// True unless the user confirmed resolution or the AI answered and the
    /// user did not say it remained open.
    pub fn is_unresolved(&self) -> bool {
        let explicit = self.updates.iter().rev().map(|u| u.kind).find(|kind| {
            matches!(
                kind,
                QuestionUpdateKind::Resolved | QuestionUpdateKind::Open
            )
        });
        match explicit {
            Some(kind) => kind == QuestionUpdateKind::Open,
            None => !self
                .updates
                .iter()
                .any(|u| u.kind == QuestionUpdateKind::Answer),
        }
    }

    pub fn status_label(&self) -> &'static str {
        let explicit = self.updates.iter().rev().map(|u| u.kind).find(|kind| {
            matches!(
                kind,
                QuestionUpdateKind::Resolved | QuestionUpdateKind::Open
            )
        });
        let status = explicit.or_else(|| {
            self.updates
                .iter()
                .any(|u| u.kind == QuestionUpdateKind::Answer)
                .then_some(QuestionUpdateKind::Answer)
        });
        match status {
            Some(QuestionUpdateKind::Answer) => {
                "Answered; resolution unconfirmed / 답변 있음, 해결 미확인"
            }
            Some(QuestionUpdateKind::Resolved) => "Resolution confirmed by user / 사용자 해결 확인",
            Some(QuestionUpdateKind::Open) => "Explicitly unresolved / 미해결 명시",
            _ => "Answer and resolution unknown / 답변 및 해결 미확인",
        }
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
    fn test_project_uses_recorded_parent_when_session_moves_into_subdirectories() {
        let root = Project::from_directories(["/a/app"]);
        assert_eq!(
            Project::from_directories(["/a/app", "/a/app/cli", "/a/app/cli/src", "/a/app"]),
            root
        );
        assert_eq!(Project::from_directories(["/a/app/cli", "/a/app"]), root);
        // Sibling directories without a recorded common parent stay unknown.
        assert!(Project::from_directories(["/a/app/cli", "/a/app/website"]).is_none());
        // A shared name prefix is not a parent directory.
        assert!(Project::from_directories(["/a/app", "/a/app-old"]).is_none());
        assert!(Project::from_directories(["/a/app", "/a/app/cli", "/b/other"]).is_none());
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
    fn test_project_folds_claude_worktree_into_its_repository() {
        let repository = Project::from_directories(["/a/app"]);
        // Shapes recorded by Claude Code: the worktree itself and directories below it.
        assert_eq!(
            Project::from_directories(["/a/app/.claude/worktrees/favicon-3d6fff"]),
            repository
        );
        assert_eq!(
            Project::from_directories([
                "/a/app/.claude/worktrees/favicon-3d6fff",
                "/a/app/.claude/worktrees/favicon-3d6fff/website",
            ]),
            repository
        );
        assert_eq!(
            Project::from_directories(["/a/app/.claude/worktrees/favicon-3d6fff/cli/src"]),
            repository
        );
        // A session that touches both the repository and its worktree stays one project.
        assert_eq!(
            Project::from_directories(["/a/app/cli", "/a/app/.claude/worktrees/x", "/a/app"]),
            repository
        );
        // A nested worktree belongs to the outermost repository.
        assert_eq!(
            Project::from_directories(["/a/app/.claude/worktrees/x/.claude/worktrees/y"]),
            repository
        );
    }

    #[test]
    fn test_project_keeps_paths_that_only_resemble_a_claude_worktree() {
        for directory in [
            // No worktree name after the marker.
            "/a/app/.claude/worktrees",
            "/a/app/.claude",
            // Similar names are not the marker.
            "/a/app/claude/worktrees/x",
            "/a/app/.claude/worktree/x",
            "/a/app/.claude-worktrees/x",
            "/a/app/.Claude/worktrees/x",
            "/a/app/worktrees/x",
            // Nothing above the marker to use as the repository.
            "/.claude/worktrees/x",
            // Worktrees outside the repository do not record where they came from.
            "/a/app-worktree",
            "/home/me/.codex/worktrees/1234/app",
        ] {
            assert_eq!(
                Project::from_directories([directory]).unwrap().directory,
                directory
            );
        }
        // The same worktree name under different repositories is not merged.
        assert_ne!(
            Project::from_directories(["/a/app/.claude/worktrees/x"]),
            Project::from_directories(["/b/app/.claude/worktrees/x"])
        );
        // Worktrees of different repositories in one session stay unknown.
        assert!(
            Project::from_directories(["/a/app/.claude/worktrees/x", "/b/app/.claude/worktrees/x"])
                .is_none()
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
