//! Runs every registered source adapter and gathers their results.

use crate::cache;
use crate::sources::{CollectContext, SourceAdapter, SourceCollection};

/// One adapter's result, tagged with the adapter's identity.
pub(super) struct CollectedSource {
    pub(super) id: &'static str,
    pub(super) display_name: &'static str,
    pub(super) collection: SourceCollection,
}

/// Results from all adapters that succeeded, plus warnings for those that failed.
pub(super) struct CollectionRun {
    pub(super) sources: Vec<CollectedSource>,
    /// (display name, error) for adapters whose `collect` returned `Err`.
    pub(super) failures: Vec<(&'static str, String)>,
}

impl CollectionRun {
    pub(super) fn session_count(&self) -> usize {
        self.sources
            .iter()
            .map(|source| source.collection.sessions.len())
            .sum()
    }

    pub(super) fn fingerprint_table(&self) -> cache::FingerprintTable {
        cache::fingerprint_table(
            self.sources
                .iter()
                .map(|source| (source.id, &source.collection.fingerprint)),
        )
    }
}

/// Calls each adapter in order. A failing adapter is recorded and skipped so
/// the remaining sources are still analyzed.
pub(super) fn collect_sources(
    adapters: &[Box<dyn SourceAdapter>],
    ctx: &CollectContext,
) -> CollectionRun {
    let mut sources = Vec::new();
    let mut failures = Vec::new();
    for adapter in adapters {
        match adapter.collect(ctx) {
            Ok(mut collection) => {
                collection.fingerprint.input_digest = Some(analysis_input_digest(&collection));
                sources.push(CollectedSource {
                    id: adapter.id(),
                    display_name: adapter.display_name(),
                    collection,
                });
            }
            Err(e) => failures.push((adapter.display_name(), e.to_string())),
        }
    }
    CollectionRun { sources, failures }
}

/// Count-only legacy fingerprints cannot match this policy. Hash only what
/// reaches analysis, not unrelated older history or file modification times.
fn analysis_input_digest(collection: &SourceCollection) -> String {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let mut sessions: Vec<_> = collection.sessions.iter().collect();
    sessions.sort_by(|a, b| a.task.session_id.cmp(&b.task.session_id));
    for session in sessions {
        session.task.session_id.hash(&mut hash);
        session.task.messages.hash(&mut hash);
        session.task.prompt_text.hash(&mut hash);
        for source in &session.task.evidence_sources {
            source.message_index.hash(&mut hash);
            source.role.hash(&mut hash);
            source.rendered_text.hash(&mut hash);
            source.location.path.hash(&mut hash);
            source.location.line.hash(&mut hash);
            source.timestamp.hash(&mut hash);
            source.is_reference.hash(&mut hash);
        }
    }
    format!("multi-day-v1:{:016x}", hash.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::SessionTask;
    use crate::sources::{CollectedSession, SourceFingerprint};

    struct FakeAdapter {
        id: &'static str,
        fail: bool,
    }

    #[test]
    fn test_cache_digest_changes_for_reference_content_and_rejects_count_only_legacy() {
        let adapter = FakeAdapter {
            id: "fake",
            fail: false,
        };
        let ctx = CollectContext {
            date: chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap(),
            config: None,
            include_automated: false,
        };
        let mut collection = adapter.collect(&ctx).unwrap();
        collection.sessions[0].task.messages = vec![
            ("USER".into(), "[REFERENCE yesterday] Use A".into()),
            ("USER".into(), "Use B".into()),
        ];
        let before = analysis_input_digest(&collection);
        collection.sessions[0].task.messages[0].1 = "[REFERENCE yesterday] Use C".into();
        let after = analysis_input_digest(&collection);
        assert_ne!(before, after);
        assert_eq!(after, analysis_input_digest(&collection));
        let cached: cache::TodayCache = serde_json::from_value(serde_json::json!({
            "date":"2099-01-01", "sources":[],
            "source_fingerprints":{"fake":{"session_count":1,"entry_count":2}},
            "timezone_window_key":cache::timezone_window_key(ctx.date)
        }))
        .unwrap();
        collection.fingerprint.input_digest = Some(after);
        let table = cache::fingerprint_table([("fake", &collection.fingerprint)]);
        assert!(!cache::fingerprints_match(&cached, &table));
        let mut fresh = cached;
        fresh.source_fingerprints = table.clone();
        assert!(cache::fingerprints_match(&fresh, &table));
    }

    #[test]
    fn test_collected_cache_identity_tracks_prior_changes_but_ignores_future_append() {
        use crate::sources::claude::ClaudeAdapter;
        let root = std::env::temp_dir().join(format!("rwd_context_digest_{}", std::process::id()));
        std::fs::create_dir_all(root.join("project")).unwrap();
        let file = root.join("project/session.jsonl");
        let old = |text| {
            serde_json::json!({"type":"user","sessionId":"s1","uuid":"old","timestamp":"2098-12-31T12:00:00Z","message":{"content":text}}).to_string()
        };
        let today = serde_json::json!({"type":"user","sessionId":"s1","uuid":"today","timestamp":"2099-01-01T12:00:00Z","message":{"content":"Use B"}}).to_string();
        let future = serde_json::json!({"type":"user","sessionId":"s1","uuid":"future","timestamp":"2099-01-02T12:00:00Z","message":{"content":"FUTURE"}}).to_string();
        let adapters: Vec<Box<dyn SourceAdapter>> =
            vec![Box::new(ClaudeAdapter::with_roots(vec![root.clone()]))];
        let ctx = CollectContext {
            date: chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap(),
            config: None,
            include_automated: false,
        };
        std::fs::write(&file, format!("{}\n{today}\n", old("Use A"))).unwrap();
        let before = collect_sources(&adapters, &ctx).fingerprint_table();
        std::fs::write(&file, format!("{}\n{today}\n", old("Use C"))).unwrap();
        let changed = collect_sources(&adapters, &ctx).fingerprint_table();
        assert_eq!(before["claude"].entry_count, changed["claude"].entry_count);
        assert_ne!(before, changed);
        std::fs::write(&file, format!("{}\n{today}\n{future}\n", old("Use C"))).unwrap();
        let appended = collect_sources(&adapters, &ctx).fingerprint_table();
        assert_eq!(changed, appended);
        std::fs::remove_dir_all(root).unwrap();
    }

    impl SourceAdapter for FakeAdapter {
        fn id(&self) -> &'static str {
            self.id
        }

        fn display_name(&self) -> &'static str {
            self.id
        }

        fn collect(
            &self,
            _ctx: &CollectContext,
        ) -> Result<SourceCollection, crate::parser::ParseError> {
            if self.fail {
                return Err("broken log root".into());
            }
            Ok(SourceCollection {
                roots: vec![std::path::PathBuf::from("/fake")],
                sessions: vec![CollectedSession {
                    task: SessionTask {
                        evidence_sources: vec![],
                        source: "fake",
                        session_id: format!("{}-session", self.id),
                        prompt_text: "[Session]".to_string(),
                        messages: Vec::new(),
                        estimated_tokens: 1,
                    },
                    first_activity: None,
                    last_activity: None,
                    token_usage: None,
                }],
                fingerprint: SourceFingerprint {
                    input_digest: None,
                    session_count: 1,
                    entry_count: 2,
                },
                ..SourceCollection::default()
            })
        }
    }

    #[test]
    fn test_collect_sources_keeps_other_sources_when_one_adapter_fails() {
        let adapters: Vec<Box<dyn SourceAdapter>> = vec![
            Box::new(FakeAdapter {
                id: "first",
                fail: false,
            }),
            Box::new(FakeAdapter {
                id: "broken",
                fail: true,
            }),
            Box::new(FakeAdapter {
                id: "third",
                fail: false,
            }),
        ];
        let ctx = CollectContext {
            date: chrono::NaiveDate::from_ymd_opt(2099, 1, 1).expect("date"),
            config: None,
            include_automated: false,
        };

        let run = collect_sources(&adapters, &ctx);

        let ids: Vec<&str> = run.sources.iter().map(|source| source.id).collect();
        assert_eq!(ids, vec!["first", "third"]);
        assert_eq!(run.session_count(), 2);
        assert_eq!(
            run.failures,
            vec![("broken", "broken log root".to_string())]
        );
        let table = run.fingerprint_table();
        assert_eq!(
            table.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["first", "third"]
        );
    }
}
