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
            Ok(collection) => sources.push(CollectedSource {
                id: adapter.id(),
                display_name: adapter.display_name(),
                collection,
            }),
            Err(e) => failures.push((adapter.display_name(), e.to_string())),
        }
    }
    CollectionRun { sources, failures }
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
