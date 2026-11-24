//! Validated per-session checkpoints, independent of the all-or-nothing daily result.

use super::{AnalysisResult, AnalyzerError};
use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize)]
struct Checkpoint {
    key: String,
    analysis: AnalysisResult,
}

pub(super) struct SessionCache {
    pub root: PathBuf,
    pub reuse: bool,
    pub scope: String,
}

impl SessionCache {
    pub fn for_date(date: chrono::NaiveDate, reuse: bool) -> Result<Self, AnalyzerError> {
        let root = dirs::home_dir()
            .ok_or(crate::messages::error::HOME_DIR_NOT_FOUND)?
            .join(".rwd/cache/sessions")
            .join(date.to_string());
        let window = crate::cache::timezone_window_key(date)
            .ok_or("Cannot determine checkpoint date window")?;
        Ok(Self {
            root,
            reuse,
            scope: format!("{date}:{window}"),
        })
    }

    pub fn key(
        &self,
        task: &super::SessionTask,
        provider: &super::provider::LlmProvider,
        lang: &crate::config::Lang,
    ) -> Result<String, AnalyzerError> {
        // Bump this policy version when validation or analysis semantics change.
        let identity = serde_json::to_vec(&(
            "session-checkpoint-v1",
            env!("CARGO_PKG_VERSION"),
            &self.scope,
            task,
            provider.cache_identity(lang),
        ))?;
        let digest = ring::digest::digest(&ring::digest::SHA256, &identity);
        Ok(digest.as_ref().iter().map(|b| format!("{b:02x}")).collect())
    }

    pub fn load(&self, key: &str) -> Option<AnalysisResult> {
        if !self.reuse {
            return None;
        }
        let bytes = std::fs::read(self.root.join(format!("{key}.json"))).ok()?;
        let saved: Checkpoint = serde_json::from_slice(&bytes).ok()?;
        (saved.key == key && !saved.analysis.sessions.is_empty()).then_some(saved.analysis)
    }

    pub fn save(&self, key: &str, analysis: &AnalysisResult) -> Result<(), AnalyzerError> {
        if analysis.sessions.is_empty() {
            return Err("Cannot checkpoint an empty analysis".into());
        }
        let mut directory = std::fs::DirBuilder::new();
        directory.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            directory.mode(0o700);
        }
        directory.create(&self.root)?;
        // Same-directory, private temporary files allow atomic replacement.
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root)?;
        serde_json::to_writer(
            &mut temporary,
            &serde_json::json!({"key": key, "analysis": analysis}),
        )?;
        temporary.as_file().sync_all()?;
        temporary.persist(self.root.join(format!("{key}.json")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_key_tracks_input_settings_and_day() {
        use super::super::provider::{LlmProvider, ProviderKind};
        use crate::config::Lang;
        let mut cache = SessionCache {
            root: PathBuf::new(),
            reuse: true,
            scope: "day1".into(),
        };
        let provider = LlmProvider::new(ProviderKind::Anthropic, true);
        let task = super::super::SessionTask {
            source: "claude",
            session_id: "s1".into(),
            project: None,
            prompt_text: String::new(),
            estimated_tokens: 1,
            messages: vec![("USER".into(), "original".into())],
            evidence_sources: vec![],
        };
        let key = cache.key(&task, &provider, &Lang::Ko).unwrap();
        assert_eq!(key, cache.key(&task, &provider, &Lang::Ko).unwrap());
        let mut changed = task.clone();
        changed.messages[0].1 = "different".into();
        assert_ne!(key, cache.key(&changed, &provider, &Lang::Ko).unwrap());
        changed = task.clone();
        changed.source = "codex";
        assert_ne!(key, cache.key(&changed, &provider, &Lang::Ko).unwrap());
        changed = task.clone();
        changed.project = super::super::retrospective::Project::from_directories(["/project"]);
        assert_ne!(key, cache.key(&changed, &provider, &Lang::Ko).unwrap());
        changed = task.clone();
        changed
            .evidence_sources
            .push(super::super::evidence::SourceMessage {
                timestamp: None,
                is_reference: false,
                message_index: 1,
                role: "USER".into(),
                text: "original".into(),
                rendered_text: "original".into(),
                location: crate::parser::SourceLocation {
                    path: "/session".into(),
                    line: 1,
                },
            });
        let located_key = cache.key(&changed, &provider, &Lang::Ko).unwrap();
        changed.evidence_sources[0].location.line = 2;
        assert_ne!(
            located_key,
            cache.key(&changed, &provider, &Lang::Ko).unwrap()
        );
        assert_ne!(key, cache.key(&task, &provider, &Lang::En).unwrap());
        assert_ne!(
            key,
            cache
                .key(
                    &task,
                    &LlmProvider::new(ProviderKind::Anthropic, false),
                    &Lang::Ko
                )
                .unwrap()
        );
        assert_ne!(
            key,
            cache
                .key(
                    &task,
                    &LlmProvider::new(ProviderKind::OpenAi, true),
                    &Lang::Ko
                )
                .unwrap()
        );
        cache.scope = "day2".into();
        assert_ne!(key, cache.key(&task, &provider, &Lang::Ko).unwrap());
        assert_eq!(key.len(), 64);
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    #[test]
    fn test_checkpoint_survives_restart_and_no_cache_bypasses_reads() {
        let root = tempfile::tempdir().unwrap();
        let mut cache = SessionCache {
            root: root.path().into(),
            reuse: true,
            scope: "day".into(),
        };
        let result: AnalysisResult = serde_json::from_value(serde_json::json!({"sessions": [{
            "session_id": "s1", "work_summary": "saved", "decisions": [],
            "curiosities": [], "corrections": []}]}))
        .unwrap();
        cache.save("key", &result).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.path().join("key.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let reopened = SessionCache {
            root: root.path().into(),
            reuse: true,
            scope: "day".into(),
        };
        assert_eq!(
            reopened.load("key").unwrap().sessions[0].work_summary,
            "saved"
        );
        cache.reuse = false;
        assert!(cache.load("key").is_none());
        cache.save("fresh", &result).unwrap();
        assert!(reopened.load("fresh").is_some());
        let mut updated = result.clone();
        updated.sessions[0].work_summary = "updated".into();
        cache.save("key", &updated).unwrap();
        assert_eq!(
            reopened.load("key").unwrap().sessions[0].work_summary,
            "updated"
        );
        assert!(
            cache
                .save("empty", &AnalysisResult { sessions: vec![] })
                .is_err()
        );
        std::fs::copy(root.path().join("key.json"), root.path().join("wrong.json")).unwrap();
        assert!(reopened.load("wrong").is_none());
        std::fs::write(root.path().join("key.json"), "unfinished").unwrap();
        assert!(reopened.load("key").is_none());
    }
}
