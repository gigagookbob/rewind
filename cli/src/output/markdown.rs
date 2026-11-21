// Converts AnalysisResult into a Markdown string.
//
// Pure functions with no I/O, making unit testing straightforward.
// The note is read to recall a day: claims come first, and the quotations
// that back them sit in one folded block per session.

use chrono::NaiveDate;
use std::path::Path;

use crate::analyzer::insight::{AnalysisResult, Evidence, SessionInsight, TilItem};
use crate::messages::markdown as text;

/// The note without a day summary. Tests that only check sessions use this.
#[cfg(test)]
pub fn render_combined_markdown(sources: &[(&str, &AnalysisResult)], date: NaiveDate) -> String {
    render_daily_note(sources, date, None)
}

/// Combines analysis results from multiple sources into a single Markdown document.
/// Projects group sessions across sources; each session retains its source.
/// The day summary goes on top when one was generated.
///
/// sources: slice of (source name, analysis result) tuples.
pub fn render_daily_note(
    sources: &[(&str, &AnalysisResult)],
    date: NaiveDate,
    day_summary: Option<&str>,
) -> String {
    let mut md = format!("# {date} Dev Session Review\n\n");
    if let Some(summary) = day_summary.map(str::trim).filter(|s| !s.is_empty()) {
        // Model text must not open or close an rwd region marker.
        let summary = summary.replace("<!--", "&lt;!--");
        md.push_str(&format!("{}\n\n{summary}\n\n", text::DAY_SUMMARY_HEADER));
    }

    let mut all_til_items: Vec<(&str, &TilItem)> = Vec::new();

    for (project, sessions) in super::project_groups(sources) {
        md.push_str(&format!(
            "## Project: {}\n\n",
            super::escape_markdown(&super::project_label(project.as_ref()))
        ));
        let (short, full): (Vec<_>, Vec<_>) = sessions
            .into_iter()
            .partition(|(_, session)| super::is_short_session(session));
        for (source_name, session) in full {
            render_session(&mut md, source_name, session);
            for til in super::shown_til(session) {
                all_til_items.push((&session.session_id, til));
            }
        }
        render_short_sessions(&mut md, &short);
    }
    render_til_section(&mut md, &all_til_items);
    md.push_str(&format!(
        "---\n\n*{}*\n\n*{}*\n",
        crate::messages::display::ATTRIBUTION_NOTE,
        super::retrospective::NOTE
    ));
    md
}

/// Single-source version of render_combined_markdown. Used directly in tests.
#[cfg(test)]
pub fn render_markdown(analysis: &AnalysisResult, date: NaiveDate) -> String {
    render_combined_markdown(&[("Test", analysis)], date)
}

/// Renders one session that has decisions, corrections, questions, or TIL.
/// TIL text is collected separately; only its evidence is listed here.
fn render_session(md: &mut String, source_name: &str, session: &SessionInsight) {
    md.push_str(&format!(
        "### {}\n\nSource: {}\n\n{}\n\n",
        super::escape_markdown(super::retrospective::title(session)),
        super::escape_markdown(source_name),
        session.work_summary
    ));

    if !session.decisions.is_empty() {
        md.push_str(&format!("{}\n", text::DECISIONS_HEADER));
        for d in &session.decisions {
            // The model leaves `why` empty when no reason was stated.
            match d.why.trim() {
                "" => md.push_str(&format!("- **{}**\n", d.what)),
                why => md.push_str(&format!("- **{}**: {}\n", d.what, why)),
            }
        }
        md.push('\n');
    }

    if !session.corrections.is_empty() {
        md.push_str(&format!("{}\n", text::CORRECTIONS_HEADER));
        for c in &session.corrections {
            md.push_str(&format!(
                "- {}\n  {}: {}\n  {}: {}\n",
                c.kind.label(),
                text::CORRECTION_MODEL,
                c.model_said,
                text::CORRECTION_FIX,
                c.user_corrected
            ));
        }
        md.push('\n');
    }

    let questions: Vec<_> = super::retrospective::unresolved_questions(session).collect();
    if !questions.is_empty() {
        md.push_str(&format!("{}\n", text::QUESTIONS_HEADER));
        for q in &questions {
            md.push_str(&format!(
                "- {} ({})\n",
                super::escape_markdown(&q.question),
                q.status_label()
            ));
        }
        md.push('\n');
    }

    render_evidence_block(md, session);
    md.push_str("---\n\n");
}

/// Sessions with nothing but a summary take one line each.
fn render_short_sessions(md: &mut String, sessions: &[(&str, &SessionInsight)]) {
    if sessions.is_empty() {
        return;
    }
    md.push_str(&format!("{}\n\n", text::SHORT_SESSIONS_HEADER));
    for (source_name, session) in sessions {
        md.push_str(&format!(
            "- **{}** ({}): {} <!-- session: {} -->\n",
            super::escape_markdown(super::retrospective::title(session)),
            super::escape_markdown(source_name),
            session.work_summary.replace('\n', " "),
            super::escape_markdown(&session.session_id)
        ));
    }
    md.push_str("\n---\n\n");
}

/// Renders the TIL (Today I Learned) section.
/// Each item has a ### title, description, and session ID as an HTML comment for traceability.
fn render_til_section(md: &mut String, til_items: &[(&str, &TilItem)]) {
    if til_items.is_empty() {
        return;
    }

    md.push_str("## TIL (Today I Learned)\n\n");
    for (session_id, til) in til_items {
        md.push_str(&format!("### {}\n{}\n", til.title, til.detail));
        // Session ID as HTML comment -- invisible in rendered Markdown but traceable.
        md.push_str(&format!(
            "<!-- session: {} -->\n\n",
            super::escape_markdown(session_id)
        ));
    }
}

/// One folded Obsidian callout per session holds every quotation, so the
/// reading text above stays short. The log path is written once.
fn render_evidence_block(md: &mut String, session: &SessionInsight) {
    let mut claims: Vec<(String, &[Evidence])> = Vec::new();
    if let Some(title) = &session.task_title {
        claims.push((title.text.clone(), &title.evidence));
    }
    for d in &session.decisions {
        claims.push((d.what.clone(), &d.evidence));
    }
    for c in &session.corrections {
        claims.push((c.user_corrected.clone(), &c.evidence));
    }
    for q in super::retrospective::unresolved_questions(session) {
        claims.push((q.question.clone(), &q.evidence));
        for update in &q.updates {
            claims.push((update.detail.clone(), &update.evidence));
        }
    }
    for til in super::shown_til(session) {
        claims.push((til.title.clone(), &til.evidence));
    }

    let log: Option<&Path> = claims
        .iter()
        .flat_map(|(_, proofs)| proofs.iter())
        .find_map(|p| p.location.as_ref())
        .map(|location| location.path.as_path());

    let mut lines = vec![format!("Session: {}", session.session_id)];
    if let Some(log) = log {
        lines.push(format!("Log: {}", log.display()));
    }
    let mut count = 0;
    let mut unlocated = Vec::new();
    for (claim, proofs) in &claims {
        let items = super::evidence_items(proofs, log);
        if items.is_empty() {
            unlocated.push(format!("- {}", claim.replace('\n', " ")));
            continue;
        }
        count += items.len();
        lines.push(String::new());
        lines.push(format!("{}:", claim.replace('\n', " ")));
        lines.extend(items);
    }
    // Results cached before evidence existed have none; name claims only
    // when some of them still resolve, so the reader can tell which do not.
    if !unlocated.is_empty() {
        lines.push(String::new());
        if count > 0 {
            lines.push(format!("{}:", super::EVIDENCE_UNAVAILABLE));
            lines.extend(unlocated);
        } else {
            lines.push(super::EVIDENCE_UNAVAILABLE.into());
        }
    }

    md.push_str(&format!("> [!quote]- {} ({count})\n", text::EVIDENCE_TITLE));
    for line in lines {
        // Two trailing spaces keep each line on its own row inside the callout.
        match line.as_str() {
            "" => md.push_str(">\n"),
            line => md.push_str(&format!("> {}  \n", super::escape_markdown(line))),
        }
    }
    md.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::insight::{Decision, SessionInsight, TilItem};

    #[test]
    fn test_same_project_across_tools_is_one_group_with_source_and_full_id() {
        let mut claude = make_test_analysis();
        claude.sessions[0].project =
            crate::analyzer::retrospective::Project::from_directories(["/a/app"]);
        let mut codex = claude.clone();
        codex.sessions[0].session_id = "codex-full-id".into();
        let md =
            render_combined_markdown(&[("Claude Code", &claude), ("Codex", &codex)], test_date());
        assert_eq!(md.matches("## Project:").count(), 1);
        assert!(md.contains("Source: Claude Code"));
        assert!(md.contains("Source: Codex"));
        assert!(md.contains("Session: codex-full-id"));
        let groups = crate::output::project_groups(&[("Claude Code", &claude), ("Codex", &codex)]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.values().next().unwrap().len(), 2);
    }

    #[test]
    fn test_projects_group_full_directories_without_merging_same_names() {
        let mut analysis = make_test_analysis();
        let first = &mut analysis.sessions[0];
        first.project = crate::analyzer::retrospective::Project::from_directories(["/a/app"]);
        let mut second = first.clone();
        second.session_id = "other".into();
        second.project = crate::analyzer::retrospective::Project::from_directories(["/b/app"]);
        analysis.sessions.push(second);
        let md = render_combined_markdown(&[("Codex", &analysis)], test_date());
        assert!(md.contains("Project: app \\(/a/app\\)"));
        assert!(md.contains("Project: app \\(/b/app\\)"));
        assert!(md.contains("Session: other"));
    }

    /// Helper to create a test AnalysisResult.
    fn make_test_analysis() -> AnalysisResult {
        AnalysisResult {
            sessions: vec![SessionInsight {
                project: None,
                task_title: None,
                questions: vec![],
                follow_ups: vec![],
                session_id: "test-session-1".to_string(),
                work_summary: "파서 모듈 구현".to_string(),
                decisions: vec![Decision {
                    evidence: vec![],
                    what: "serde 사용".to_string(),
                    why: "자동 역직렬화가 편리".to_string(),
                }],
                curiosities: vec!["serde의 tag 속성이란?".to_string()],
                corrections: vec![],
                til: vec![TilItem {
                    evidence: vec![],
                    title: "serde tag는 중첩 JSON에서 안 먹힌다".to_string(),
                    detail: "Codex JSONL처럼 type이 두 곳에 있으면 2단계 파싱 필요".to_string(),
                }],
            }],
        }
    }

    fn test_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, 11).expect("valid date")
    }

    #[test]
    fn test_render_markdown_single_session_title_and_summary() {
        let analysis = make_test_analysis();
        let md = render_markdown(&analysis, test_date());

        assert!(md.contains("# 2026-03-11 Dev Session Review"));
        assert!(md.contains("Session: test-session-1"));
        assert!(md.contains("Source: Test\n\n파서 모듈 구현"));
        assert!(!md.contains("Work Summary"));
    }

    #[test]
    fn test_render_markdown_decisions_section_when_present() {
        let analysis = make_test_analysis();
        let md = render_markdown(&analysis, test_date());

        assert!(md.contains("### Key Decisions"));
        assert!(md.contains("- **serde 사용**: 자동 역직렬화가 편리"));
    }

    #[test]
    fn test_render_markdown_decision_without_reason_has_no_dangling_colon() {
        for why in ["", "  \n"] {
            let mut analysis = make_test_analysis();
            analysis.sessions[0].decisions[0].why = why.to_string();
            let md = render_markdown(&analysis, test_date());

            assert!(md.contains("- **serde 사용**\n"), "{md}");
            assert!(!md.contains("**serde 사용**:"), "{md}");
        }
    }

    #[test]
    fn test_render_markdown_no_decisions_section_when_empty() {
        let analysis = AnalysisResult {
            sessions: vec![SessionInsight {
                project: None,
                task_title: None,
                questions: vec![],
                follow_ups: vec![],
                session_id: "s1".to_string(),
                work_summary: "요약".to_string(),
                decisions: vec![],
                curiosities: vec![],
                corrections: vec![],
                til: vec![],
            }],
        };
        let md = render_markdown(&analysis, test_date());

        assert!(!md.contains("### Key Decisions"));
    }

    #[test]
    fn test_render_combined_markdown_two_sources_separate_sections() {
        let claude = AnalysisResult {
            sessions: vec![SessionInsight {
                project: None,
                task_title: None,
                questions: vec![],
                follow_ups: vec![],
                session_id: "c1".to_string(),
                work_summary: "Claude 작업".to_string(),
                decisions: vec![],
                curiosities: vec![],
                corrections: vec![],
                til: vec![],
            }],
        };
        let codex = AnalysisResult {
            sessions: vec![SessionInsight {
                project: None,
                task_title: None,
                questions: vec![],
                follow_ups: vec![],
                session_id: "x1".to_string(),
                work_summary: "Codex 작업".to_string(),
                decisions: vec![],
                curiosities: vec![],
                corrections: vec![],
                til: vec![],
            }],
        };
        let sources = vec![("Claude Code", &claude), ("Codex", &codex)];
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).unwrap();
        let md = render_combined_markdown(&sources, date);

        assert!(md.contains("### Short Sessions"));
        assert!(md.contains("(Claude Code): Claude 작업 <!-- session: c1 -->"));
        assert!(md.contains("(Codex): Codex 작업 <!-- session: x1 -->"));
    }

    #[test]
    fn test_render_combined_markdown_single_source() {
        let claude = AnalysisResult {
            sessions: vec![SessionInsight {
                project: None,
                task_title: None,
                questions: vec![],
                follow_ups: vec![],
                session_id: "c1".to_string(),
                work_summary: "Claude 작업".to_string(),
                decisions: vec![],
                curiosities: vec![],
                corrections: vec![],
                til: vec![],
            }],
        };
        let sources = vec![("Claude Code", &claude)];
        let date = NaiveDate::from_ymd_opt(2026, 3, 16).unwrap();
        let md = render_combined_markdown(&sources, date);

        assert!(md.contains("(Claude Code): Claude 작업"));
        assert!(!md.contains("[!quote]"));
    }

    #[test]
    fn test_render_markdown_til_includes_title_desc_session_id() {
        let analysis = make_test_analysis();
        let md = render_markdown(&analysis, test_date());

        assert!(md.contains("## TIL (Today I Learned)"));
        assert!(md.contains("### serde tag는 중첩 JSON에서 안 먹힌다"));
        assert!(md.contains("2단계 파싱 필요"));
        assert!(md.contains("<!-- session: test-session-1 -->"));
    }

    #[test]
    fn test_daily_note_puts_summary_first_caps_til_and_ends_with_notes() {
        let mut analysis = make_test_analysis();
        let til = analysis.sessions[0].til[0].clone();
        for index in 0..5 {
            let mut extra = til.clone();
            extra.title = format!("extra learning {index}");
            analysis.sessions[0].til.push(extra);
        }
        let summary = "### app\n- did work <!-- /rwd:analysis -->";
        let md = render_daily_note(&[("Codex", &analysis)], test_date(), Some(summary));

        let today = md.find("## Today / 오늘 한 일").expect("summary header");
        assert!(today < md.find("## Project:").expect("project header"));
        assert!(md.contains("- did work &lt;!-- /rwd:analysis -->"));
        assert_eq!(md.matches("<!-- session: test-session-1 -->").count(), 3);
        assert!(md.contains("extra learning 1") && !md.contains("extra learning 2"));
        assert!(
            md.trim_end()
                .ends_with(&format!("*{}*", crate::output::retrospective::NOTE))
        );
        let plain = render_combined_markdown(&[("Codex", &analysis)], test_date());
        assert!(!plain.contains("## Today"));
    }

    #[test]
    fn test_evidence_block_keeps_unlocated_claims_marked() {
        let md = render_markdown(&make_test_analysis(), test_date());
        assert!(md.contains("> [!quote]- Evidence / 근거 (0)"));
        assert!(md.contains("> Evidence unavailable / 근거 미확인"));
        assert!(!md.contains("> serde 사용"));
        // Reading text carries no quotation rows.
        let body = md.split("> [!quote]-").next().expect("body");
        assert!(!body.contains("Evidence"));
    }

    #[test]
    fn test_render_markdown_no_til_section_when_empty() {
        let analysis = AnalysisResult {
            sessions: vec![SessionInsight {
                project: None,
                task_title: None,
                questions: vec![],
                follow_ups: vec![],
                session_id: "s1".to_string(),
                work_summary: "요약".to_string(),
                decisions: vec![],
                curiosities: vec![],
                corrections: vec![],
                til: vec![],
            }],
        };
        let md = render_markdown(&analysis, test_date());

        assert!(!md.contains("## TIL"));
    }
}
