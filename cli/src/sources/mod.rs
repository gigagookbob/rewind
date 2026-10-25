//! Per-agent log adapters.
//!
//! Each adapter turns one agent's local logs into analysis-ready
//! `SessionTask`s. Commands, the cache, and `rwd doctor` only walk the
//! adapter list, so adding an agent means adding one adapter here.

pub mod antigravity;
pub mod claude;
pub mod codex;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::analyzer::SessionTask;
use crate::config::Config;
use crate::parser::ParseError;

/// One agent's log source. Adapters only read files; they never write.
pub trait SourceAdapter {
    /// Stable id used in config and cache keys, e.g. "antigravity".
    fn id(&self) -> &'static str;
    /// Heading shown in the terminal and Markdown, e.g. "Antigravity".
    fn display_name(&self) -> &'static str;
    /// Collects sessions with activity inside the local date window.
    fn collect(&self, ctx: &CollectContext) -> Result<SourceCollection, ParseError>;
}

/// Inputs shared by every adapter for one collection run.
pub struct CollectContext<'a> {
    pub date: NaiveDate,
    pub config: Option<&'a Config>,
    /// `--include-automated` flag. Adapters may also read their own config.
    pub include_automated: bool,
}

/// Everything one adapter found for the target date.
#[derive(Debug, Clone, Default)]
pub struct SourceCollection {
    /// Log roots that exist on this machine (for `-v`, `--dry-run`, `rwd doctor`).
    pub roots: Vec<PathBuf>,
    /// Ready-to-analyze sessions, sorted by first activity.
    pub sessions: Vec<CollectedSession>,
    /// Sessions dropped on purpose, by reason (e.g. "print-mode", "subagent").
    pub excluded: Vec<(&'static str, usize)>,
    /// Counts compared against the cache to decide whether to re-analyze.
    pub fingerprint: SourceFingerprint,
    /// Extra lines printed only with `-v`.
    pub verbose_lines: Vec<String>,
}

/// One session ready for analysis, with display metadata.
#[derive(Debug, Clone)]
pub struct CollectedSession {
    pub task: SessionTask,
    pub first_activity: Option<DateTime<Utc>>,
    pub last_activity: Option<DateTime<Utc>>,
    /// (input, output) totals when the source records token usage.
    pub token_usage: Option<(u64, u64)>,
}

/// Per-source counts stored in the cache to detect new activity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFingerprint {
    pub session_count: usize,
    pub entry_count: usize,
}

/// Returns every adapter in display order: Claude Code, Codex, Antigravity.
pub fn registered_adapters() -> Vec<Box<dyn SourceAdapter>> {
    vec![
        Box::new(claude::ClaudeAdapter::default()),
        Box::new(codex::CodexAdapter::default()),
        Box::new(antigravity::AntigravityAdapter::default()),
    ]
}
