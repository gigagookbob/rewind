// Sends parsed log data to LLM APIs and extracts developer insights.

pub mod anthropic;
pub mod codex_exec;
pub mod compaction;
mod executor;
pub mod insight;
pub mod openai;
pub mod planner;
pub mod prompt;
pub mod provider;
pub mod summarizer;

// TODO: Replace with a dedicated error type via thiserror.
pub type AnalyzerError = Box<dyn std::error::Error>;

// Re-export commonly used types at the module level.
pub use insight::AnalysisResult;

/// Token usage returned by LLM API responses.
#[derive(Debug, Default)]
pub struct ApiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

use crate::parser::claude::LogEntry;
use crate::parser::codex::CodexEntry;
use crate::redactor::RedactResult;
use executor::execute_plan_parallel;
use planner::{SessionEstimate, StepStrategy};
use std::collections::HashMap;

/// Common per-session analysis input shared by every log source.
#[derive(Debug, Clone)]
pub struct SessionTask {
    /// Id of the source adapter that built this task (e.g. "claude"), for `-v` output.
    pub source: &'static str,
    pub session_id: String,
    pub prompt_text: String,
    pub messages: Vec<(String, String)>,
    pub estimated_tokens: u64,
}

/// Shared provider/rate-limit context initialized once per `rwd today` run.
pub struct SessionAnalysisContext {
    provider: provider::LlmProvider,
    api_key: String,
    rate_limits: planner::RateLimits,
}

/// Starts a terminal spinner on stderr.
fn start_spinner(msg: String) -> indicatif::ProgressBar {
    use indicatif::{ProgressBar, ProgressStyle};
    use std::time::Duration;

    let spinner = ProgressBar::new_spinner();
    let style = ProgressStyle::with_template("{spinner} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_spinner())
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""]);
    spinner.set_style(style);
    spinner.set_message(msg);
    spinner.enable_steady_tick(Duration::from_millis(80));
    spinner
}

/// Stops the spinner and clears the line.
fn stop_spinner(spinner: indicatif::ProgressBar) {
    spinner.finish_and_clear();
}

fn print_rate_limit_status(
    provider: &provider::LlmProvider,
    limits: &planner::RateLimits,
    probed: bool,
) {
    if probed {
        eprintln!(
            "{}",
            crate::messages::status::rate_limit_ok(
                limits.input_tokens_per_minute,
                limits.output_tokens_per_minute,
                limits.requests_per_minute,
            )
        );
    } else if provider.supports_rate_limit_probe() {
        eprintln!(
            "{}",
            crate::messages::status::rate_limit_fallback(
                limits.input_tokens_per_minute,
                limits.output_tokens_per_minute,
                limits.requests_per_minute,
            )
        );
    } else {
        eprintln!(
            "{}",
            crate::messages::status::rate_limit_probe_skipped(
                provider.display_name(),
                limits.input_tokens_per_minute,
                limits.output_tokens_per_minute,
                limits.requests_per_minute,
            )
        );
    }
}

/// Loads provider and probes rate limits once for a full `today` analysis run.
pub async fn prepare_session_analysis_context() -> Result<SessionAnalysisContext, AnalyzerError> {
    let (provider, api_key) = provider::load_provider()?;
    let sp = start_spinner(crate::messages::status::PROBING_RATE_LIMITS.into());
    let (rate_limits, probed) = provider.probe_rate_limits(&api_key).await;
    stop_spinner(sp);
    print_rate_limit_status(&provider, &rate_limits, probed);

    Ok(SessionAnalysisContext {
        provider,
        api_key,
        rate_limits,
    })
}

/// Displays a countdown with spinner animation, updating every second.
async fn countdown_sleep(total_secs: u64) {
    use std::time::Duration;

    let spinner = start_spinner(crate::messages::status::countdown_waiting(total_secs));
    for remaining in (1..=total_secs).rev() {
        spinner.set_message(crate::messages::status::countdown_waiting(remaining));
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    stop_spinner(spinner);
}

/// Extracts session_id from a LogEntry (None for FileHistorySnapshot and Other).
fn entry_session_id(entry: &LogEntry) -> Option<&str> {
    match entry {
        LogEntry::User(e) => Some(&e.session_id),
        LogEntry::Assistant(e) => Some(&e.session_id),
        LogEntry::Progress(e) => Some(&e.session_id),
        LogEntry::System(e) => e.session_id.as_deref(),
        LogEntry::FileHistorySnapshot(_) | LogEntry::Other(_) => None,
    }
}

/// Builds common session tasks from Claude entries. `lines_by_session` holds
/// extra lines (e.g. subagent results) to splice into each session by time.
pub fn build_claude_session_tasks(
    entries: &[LogEntry],
    source: &'static str,
    lines_by_session: &HashMap<String, Vec<prompt::TimedLine>>,
) -> Result<Vec<SessionTask>, AnalyzerError> {
    let session_ids = prompt::extract_session_ids(entries);
    if session_ids.is_empty() {
        return Err(crate::messages::error::NO_CONVERSATION_CLAUDE.into());
    }

    let estimated_by_session: HashMap<String, u64> = prompt::estimate_sessions(entries)
        .into_iter()
        .map(|estimate| (estimate.session_id, estimate.estimated_tokens))
        .collect();

    let mut tasks = Vec::new();
    for session_id in session_ids {
        let session_entries: Vec<LogEntry> = entries
            .iter()
            .filter(|entry| entry_session_id(entry) == Some(session_id.as_str()))
            .cloned()
            .collect();

        let lines = lines_by_session
            .get(&session_id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let prompt_text = prompt::build_prompt_with_lines(&session_entries, lines)?;
        let messages = prompt::extract_messages_with_lines(&session_entries, lines);
        let estimated_tokens = estimated_by_session
            .get(&session_id)
            .map(|estimate| estimate + prompt::estimate_line_tokens(lines))
            .unwrap_or_else(|| prompt::estimate_prompt_tokens(&prompt_text));

        tasks.push(SessionTask {
            source,
            session_id,
            prompt_text,
            messages,
            estimated_tokens,
        });
    }

    Ok(tasks)
}

/// Builds one common session task from Codex entries, splicing `lines`
/// (e.g. subagent results) in by time.
pub fn build_codex_session_task(
    entries: &[CodexEntry],
    session_id: &str,
    source: &'static str,
    lines: &[prompt::TimedLine],
) -> Result<SessionTask, AnalyzerError> {
    let prompt_text = prompt::build_codex_prompt_with_lines(entries, session_id, lines)?;
    let messages = prompt::extract_codex_messages_with_lines(entries, lines);
    let estimated_tokens = prompt::estimate_prompt_tokens(&prompt_text);

    Ok(SessionTask {
        source,
        session_id: session_id.to_string(),
        prompt_text,
        messages,
        estimated_tokens,
    })
}

/// Shared session analysis pipeline used by both Claude and Codex inputs.
pub async fn analyze_session_tasks(
    tasks: Vec<SessionTask>,
    redactor_enabled: bool,
    verbose: bool,
    lang: &crate::config::Lang,
    scope_label: &str,
    context: &SessionAnalysisContext,
) -> Result<(AnalysisResult, RedactResult), AnalyzerError> {
    if tasks.is_empty() {
        return Err(crate::messages::error::NO_SESSIONS.into());
    }

    eprintln!(
        "{}",
        crate::messages::status::analyzing_source(scope_label, tasks.len())
    );

    if verbose {
        let mut per_source: std::collections::BTreeMap<&str, usize> =
            std::collections::BTreeMap::new();
        for task in &tasks {
            *per_source.entry(task.source).or_default() += 1;
        }
        let counts = per_source
            .iter()
            .map(|(source, count)| format!("{source} {count}"))
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("• Session tasks: {counts}");
    }

    // Build execution plan from unified tasks.
    let estimates: Vec<SessionEstimate> = tasks
        .iter()
        .map(|task| SessionEstimate {
            session_id: task.session_id.clone(),
            estimated_tokens: task.estimated_tokens,
        })
        .collect();
    let plan = planner::build_execution_plan(
        &context.rate_limits,
        &estimates,
        context.provider.max_output_tokens(),
    );

    // 3. Display plan
    eprintln!(
        "{}",
        crate::messages::status::plan_multi_step_scoped(
            scope_label,
            plan.steps.len(),
            plan.total_estimated_tokens
        )
    );
    if verbose {
        for step in &plan.steps {
            match &step.strategy {
                StepStrategy::Direct => {
                    eprintln!(
                        "{}",
                        crate::messages::status::plan_step_direct(
                            &step.session_id,
                            step.estimated_tokens,
                        )
                    );
                }
                StepStrategy::Summarize { chunks } => {
                    eprintln!(
                        "{}",
                        crate::messages::status::plan_step_summarize(
                            &step.session_id,
                            step.estimated_tokens,
                            *chunks,
                        )
                    );
                }
            }
        }
    }

    execute_plan_parallel(
        &plan,
        tasks,
        &context.provider,
        &context.api_key,
        redactor_enabled,
        verbose,
        lang,
    )
    .await
}

/// Generates a development progress summary from concatenated session work_summaries.
pub async fn analyze_summary(
    session_summaries: &str,
    lang: &crate::config::Lang,
) -> Result<String, AnalyzerError> {
    let (provider, api_key) = provider::load_provider()?;
    let (raw_response, _usage) = provider
        .call_summary_api(&api_key, session_summaries, lang)
        .await?;
    Ok(raw_response)
}

/// Generates a Slack-ready message from concatenated session work_summaries.
pub async fn analyze_slack(
    session_summaries: &str,
    lang: &crate::config::Lang,
) -> Result<String, AnalyzerError> {
    let (provider, api_key) = provider::load_provider()?;
    let (raw_response, _usage) = provider
        .call_slack_api(&api_key, session_summaries, lang)
        .await?;
    Ok(raw_response)
}
