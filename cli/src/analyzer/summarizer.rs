// Large session chunking and summarization module.
//
// Splits sessions exceeding ITPM into message-level chunks,
// summarizes each chunk via the LLM, then combines results.

use super::evidence::InputFragment;
use super::planner::RateLimits;
use super::prompt::estimate_tokens;

use crate::config::Lang;

/// Safety cap on the number of chunk-summarize API calls we make for a single
/// session. A pathological session that still explodes into dozens of chunks
/// after compaction would otherwise burn through the provider's daily quota.
pub const MAX_CHUNKS_PER_SESSION: usize = 3;
// Application ceiling, independent of a provider's potentially generous rate limit.
const MAX_CHUNK_INPUT_TOKENS: u64 = 30_000;
const CHUNK_HEADER_RESERVE_CHARS: usize = 128;

const CHUNK_SUMMARIZE_PROMPT_EN: &str = include_str!("../../prompts/chunk_summarize_en.md");
const CHUNK_SUMMARIZE_PROMPT_KO: &str = include_str!("../../prompts/chunk_summarize_ko.md");

pub type Chunk = Vec<InputFragment>;

struct SummaryInputs {
    inputs: Vec<(usize, String)>,
    coverage_notice: Option<String>,
    source_fragments: Chunk,
}

pub struct ChunkSummary {
    pub text: String,
    pub coverage_notice: Option<String>,
    pub source_fragments: Chunk,
}

fn selected_chunk_indices(total: usize) -> Vec<usize> {
    if total <= MAX_CHUNKS_PER_SESSION {
        (0..total).collect()
    } else {
        // Keep a contiguous tail window, not just the final message.
        vec![0, total - 2, total - 1]
    }
}

fn prepare_summary_inputs(
    chunks: &[Chunk],
    lang: &Lang,
) -> Result<SummaryInputs, super::AnalyzerError> {
    if chunks.is_empty() {
        return Err("No conversation chunks to summarize".into());
    }
    let coverage_notice = (chunks.len() > MAX_CHUNKS_PER_SESSION)
        .then(|| crate::messages::status::chunk_coverage(chunks.len(), lang));
    let inputs = selected_chunk_indices(chunks.len())
        .into_iter()
        .map(|index| {
            let mut text = format!(
                "[Source chunk {}/{}; chronological order]\n",
                index + 1,
                chunks.len()
            );
            for fragment in &chunks[index] {
                text.push_str(&format!("[{}] {}\n", fragment.role, fragment.text));
            }
            (index + 1, text)
        })
        .collect();
    Ok(SummaryInputs {
        inputs,
        coverage_notice,
        source_fragments: selected_chunk_indices(chunks.len())
            .into_iter()
            .flat_map(|index| chunks[index].iter().cloned())
            .collect(),
    })
}

fn combine_summaries(
    plan: &SummaryInputs,
    summaries: Vec<String>,
    total: usize,
) -> Result<ChunkSummary, super::AnalyzerError> {
    if summaries.len() != plan.inputs.len() || summaries.iter().any(|s| s.trim().is_empty()) {
        return Err("Missing or empty source chunk summary; analysis was not completed".into());
    }
    let mut sections = Vec::new();
    if let Some(notice) = &plan.coverage_notice {
        sections.push(notice.clone());
    }
    for ((source_number, _), summary) in plan.inputs.iter().zip(summaries) {
        sections.push(format!(
            "[Synthetic summary of source chunk {source_number}/{total}]\n{summary}"
        ));
    }
    Ok(ChunkSummary {
        text: sections.join("\n\n"),
        coverage_notice: plan.coverage_notice.clone(),
        source_fragments: plan.source_fragments.clone(),
    })
}

pub fn get_chunk_summarize_prompt(lang: &Lang) -> &'static str {
    match lang {
        Lang::En => CHUNK_SUMMARIZE_PROMPT_EN,
        Lang::Ko => CHUNK_SUMMARIZE_PROMPT_KO,
    }
}

/// Reserve space before splitting so headers never push a full chunk over budget.
pub fn chunk_input_budgets(
    itpm: u64,
    max_conversation_chars: usize,
    lang: &Lang,
) -> Result<(u64, usize), super::AnalyzerError> {
    let overhead = estimate_tokens(get_chunk_summarize_prompt(lang))
        + CHUNK_HEADER_RESERVE_CHARS as u64 / 2
        + 1;
    let tokens = itpm
        .min(MAX_CHUNK_INPUT_TOKENS)
        .checked_sub(overhead)
        .filter(|&n| n > 0)
        .ok_or("Chunk input budget cannot fit the summarization prompt")?;
    let chars = max_conversation_chars
        .checked_sub(CHUNK_HEADER_RESERVE_CHARS)
        .filter(|&n| n > 0)
        .ok_or("Chunk input budget cannot fit the source chunk header")?;
    Ok((tokens, chars))
}

/// Splits messages into chunks that fit within the ITPM limit.
/// Splits at message boundaries when possible, or within a message to satisfy
/// both the estimated token budget and provider character cap.
pub fn split_into_chunks(
    messages: &[(String, String)],
    itpm: u64,
    max_chunk_chars: usize,
) -> Result<Vec<Chunk>, super::AnalyzerError> {
    if messages.is_empty() {
        return Ok(Vec::new());
    }

    let mut chunks = Vec::new();
    let mut current_chunk = Vec::new();
    let mut current_chars: usize = 0;
    // Bound even a single huge message, including role and evidence ID overhead.
    let token_chars = usize::try_from(itpm.saturating_mul(2)).unwrap_or(usize::MAX);
    let max_chunk_chars = max_chunk_chars.min(token_chars);

    for (index, (role, text)) in messages.iter().enumerate() {
        let parts = split_message_text_by_chars(role, text, max_chunk_chars)?;
        for part in parts {
            let msg_chars = rendered_message_chars(role, &part);

            // Adding this message would exceed a limit -- start a new chunk.
            if !current_chunk.is_empty()
                && current_chars.saturating_add(msg_chars) > max_chunk_chars
            {
                chunks.push(current_chunk);
                current_chunk = Vec::new();
                current_chars = 0;
            }

            current_chunk.push(InputFragment {
                message_index: index + 1,
                role: role.clone(),
                text: part,
            });
            current_chars = current_chars.saturating_add(msg_chars);
        }
    }

    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    Ok(chunks)
}

fn rendered_message_chars(role: &str, text: &str) -> usize {
    // Render format is "[ROLE] {text}\n".
    role.chars().count() + 4 + text.chars().count()
}

fn split_message_text_by_chars(
    role: &str,
    text: &str,
    max_chunk_chars: usize,
) -> Result<Vec<String>, super::AnalyzerError> {
    let role_overhead = role.chars().count() + 4;
    let max_text_chars = max_chunk_chars
        .checked_sub(role_overhead)
        .ok_or("Chunk input budget cannot fit the message role")?;
    let total_chars = text.chars().count();
    if total_chars <= max_text_chars {
        return Ok(vec![text.to_string()]);
    }

    // annotate_messages owns these IDs. Repeating an ID identifies fragments of
    // the same original speech; it never gives a synthetic summary a new ID.
    let prefix = evidence_prefix(text);
    let payload = &text[prefix.len()..];
    let part_chars = max_text_chars
        .checked_sub(prefix.chars().count())
        .filter(|&n| n > 0)
        .ok_or("Chunk input budget cannot fit the message ID and content")?;

    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut count = 0usize;
    for (idx, _) in payload.char_indices() {
        if count == part_chars {
            parts.push(format!("{prefix}{}", &payload[start..idx]));
            start = idx;
            count = 0;
        }
        count += 1;
    }
    if start < payload.len() {
        parts.push(format!("{prefix}{}", &payload[start..]));
    }
    Ok(parts)
}

fn evidence_prefix(text: &str) -> &str {
    if let Some((id, _)) = text.split_once("] ")
        && let Some(number) = id.strip_prefix("[m")
        && !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit())
    {
        &text[..id.len() + 2]
    } else {
        ""
    }
}

/// Calculates wait time based on ITPM/RPM limits.
/// Returns max(itpm_wait, rpm_wait).
pub fn calculate_wait(used_tokens: u64, limits: &RateLimits) -> f64 {
    let itpm_wait = (used_tokens as f64 / limits.input_tokens_per_minute.max(1) as f64) * 60.0;
    let rpm_wait = 60.0 / limits.requests_per_minute.max(1) as f64;
    itpm_wait.max(rpm_wait)
}

/// Summarizes a large session's chunks and returns the combined summary text.
/// Applies rate pacing between chunk API calls.
pub async fn summarize_chunks(
    chunks: &[Chunk],
    provider: &super::provider::LlmProvider,
    api_key: &str,
    limits: &RateLimits,
    lang: &Lang,
) -> Result<ChunkSummary, super::AnalyzerError> {
    let plan = prepare_summary_inputs(chunks, lang)?;
    if let Some(notice) = &plan.coverage_notice {
        eprintln!("{notice}");
    }
    let mut summaries = Vec::new();
    for (source_number, chunk_text) in &plan.inputs {
        let sp = super::start_spinner(crate::messages::status::chunk_summarizing(
            *source_number,
            chunks.len(),
        ));

        let response = provider
            .call_text_api(api_key, get_chunk_summarize_prompt(lang), chunk_text, 2000)
            .await;
        super::stop_spinner(sp);
        let (summary, _usage) = response?;
        if summary.trim().is_empty() {
            return Err(format!("Empty summary for source chunk {source_number}").into());
        }
        eprintln!(
            "{}",
            crate::messages::status::chunk_done(*source_number, chunks.len())
        );
        summaries.push(summary);

        // The last chunk is also followed by an API call: final analysis.
        let chunk_tokens =
            estimate_tokens(chunk_text) + estimate_tokens(get_chunk_summarize_prompt(lang));
        let wait = calculate_wait(chunk_tokens, limits);
        if wait > 0.0 {
            super::countdown_sleep(wait.ceil() as u64).await;
        }
    }

    combine_summaries(&plan, summaries, chunks.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEST_MAX_CHARS: usize = 1_048_576;

    fn fragment(message_index: usize, role: &str, text: &str) -> InputFragment {
        InputFragment {
            message_index,
            role: role.into(),
            text: text.into(),
        }
    }

    #[test]
    fn test_summary_cannot_cite_an_omitted_original_message() {
        use super::super::{evidence, insight};
        let messages: Vec<(String, String)> = (1..=4)
            .map(|i| ("USER".into(), format!("decision-{i}")))
            .collect();
        let sources = messages
            .iter()
            .enumerate()
            .map(|(i, (role, text))| evidence::SourceMessage {
                message_index: i + 1,
                role: role.clone(),
                text: text.clone(),
                rendered_text: text.clone(),
                location: crate::parser::SourceLocation {
                    path: "synthetic.jsonl".into(),
                    line: i + 1,
                },
            })
            .collect::<Vec<_>>();
        let (annotated, catalog) = evidence::annotate_messages(&messages, &sources);
        let chunks = split_into_chunks(&annotated, 100, 24).unwrap();
        assert_eq!(chunks.len(), 4);
        let plan = prepare_summary_inputs(&chunks, &Lang::En).unwrap();
        assert!(
            !plan
                .inputs
                .iter()
                .any(|(_, input)| input.contains("decision-2"))
        );
        let mut result = insight::parse_response(
            r#"{"sessions":[{
            "session_id":"s1","work_summary":"partial","decisions":[{
                "what":"hidden","evidence":[{"message_id":"m2","quote":"decision-2"}]
            }],"curiosities":[],"corrections":[],"til":[]
        }]}"#,
        )
        .unwrap();
        evidence::validate_summary(&mut result, "s1", &catalog, &plan.source_fragments, false);
        assert!(result.sessions[0].decisions.is_empty());
    }

    #[test]
    fn test_long_session_tail_keeps_source_locations_and_rejects_synthetic_evidence() {
        use super::super::{evidence, insight};
        let messages: Vec<(String, String)> = vec![
            ("USER".into(), "A를 선택합니다".into()),
            ("ASSISTANT".into(), "빌드가 실패했습니다".into()),
            ("META".into(), "중간 작업 ".repeat(300)),
            ("ASSISTANT".into(), "A를 계속 사용합니다".into()),
            (
                "USER".into(),
                "B로 바꿉니다. 빌드 문제는 해결했습니다".into(),
            ),
            ("ASSISTANT".into(), "테스트는 여전히 실패했습니다".into()),
            ("USER".into(), "정정합니다. 테스트도 통과했습니다".into()),
        ];
        let sources: Vec<evidence::SourceMessage> = messages
            .iter()
            .enumerate()
            .filter(|(_, (role, _))| role != "META")
            .map(|(i, (role, text))| evidence::SourceMessage {
                message_index: i + 1,
                role: role.clone(),
                text: text.clone(),
                rendered_text: text.clone(),
                location: crate::parser::SourceLocation {
                    path: "synthetic.jsonl".into(),
                    line: i + 11,
                },
            })
            .collect();
        let (annotated, catalog) = evidence::annotate_messages(&messages, &sources);
        let chunks = split_into_chunks(&annotated, 100, 200).unwrap();
        assert!(chunks.len() >= 4);
        let plan = prepare_summary_inputs(&chunks, &Lang::Ko).unwrap();
        let echoed = plan.inputs.iter().map(|(_, text)| text.clone()).collect();
        let summary = combine_summaries(&plan, echoed, chunks.len()).unwrap();
        for id in ["[m1]", "[m5]", "[m6]", "[m7]"] {
            assert!(summary.text.contains(id), "missing {id}");
        }
        assert!(!summary.text.contains("synthetic.jsonl"));
        let mut result = insight::parse_response(r#"{"sessions":[{
            "session_id":"s1","work_summary":"B를 사용합니다",
            "decisions":[
                {"what":"B","evidence":[{"message_id":"m5","quote":"B로 바꿉니다"}]},
                {"what":"synthetic","evidence":[{"message_id":"m7","quote":"합성 요약에서 만든 문장"}]}
            ],"curiosities":[],"til":[],"corrections":[
                {"kind":"factual_error","evidence":[
                    {"message_id":"m6","quote":"테스트는 여전히 실패했습니다"},
                    {"message_id":"m7","quote":"정정합니다. 테스트도 통과했습니다"}]},
                {"kind":"preference_change","evidence":[
                    {"message_id":"m6","quote":"테스트는 여전히 실패했습니다"},
                    {"message_id":"m1","quote":"A를 선택합니다"}]}
            ]}] }"#).unwrap();
        evidence::validate_summary(
            &mut result,
            "s1",
            &catalog,
            &summary.source_fragments,
            false,
        );
        let session = &result.sessions[0];
        assert_eq!(session.decisions.len(), 1);
        assert_eq!(
            session.decisions[0].evidence[0]
                .location
                .as_ref()
                .unwrap()
                .line,
            15
        );
        assert_eq!(session.corrections.len(), 1);
        assert_eq!(
            session.corrections[0].kind,
            insight::CorrectionKind::FactualError
        );
        assert_eq!(
            session.corrections[0].evidence[1]
                .location
                .as_ref()
                .unwrap()
                .line,
            17
        );
    }

    #[test]
    fn test_selection_keeps_head_and_adjacent_tail_in_order_with_fixed_call_cap() {
        for total in [0, 1, 2, 3, 4, 5, 100, 10_000] {
            let selected = selected_chunk_indices(total);
            assert_eq!(selected.len(), total.min(MAX_CHUNKS_PER_SESSION));
            assert!(selected.windows(2).all(|w| w[0] < w[1]));
            if total <= MAX_CHUNKS_PER_SESSION {
                assert_eq!(selected, (0..total).collect::<Vec<_>>());
            } else {
                assert_eq!(selected, vec![0, total - 2, total - 1]);
            }
        }
    }

    #[test]
    fn test_chunk_budgets_include_prompt_headers_and_local_input_ceiling() {
        for lang in [Lang::Ko, Lang::En] {
            for (itpm, cap) in [
                (2_000, usize::MAX),
                (1_000_000, 1_000),
                (1_000_000, usize::MAX),
            ] {
                let (tokens, chars) = chunk_input_budgets(itpm, cap, &lang).unwrap();
                assert!(tokens < itpm.min(MAX_CHUNK_INPUT_TOKENS));
                assert!(chars < cap);
                let messages = vec![("USER".into(), format!("[m1] {}", "가🙂".repeat(40_000)))];
                let chunks = split_into_chunks(&messages, tokens, chars).unwrap();
                let plan = prepare_summary_inputs(&chunks, &lang).unwrap();
                for (_, input) in plan.inputs {
                    assert!(input.chars().count() <= cap);
                    let combined = format!("{}\n{input}", get_chunk_summarize_prompt(&lang));
                    assert!(estimate_tokens(&combined) <= itpm.min(MAX_CHUNK_INPUT_TOKENS));
                }
            }
            assert!(chunk_input_budgets(1, 1_000, &lang).is_err());
            assert!(chunk_input_budgets(30_000, 1, &lang).is_err());
        }
    }

    #[test]
    fn test_summary_combination_keeps_source_order_labels_and_omission_notice() {
        let chunks: Vec<Chunk> = (1..=5)
            .map(|i| {
                vec![InputFragment {
                    message_index: i,
                    role: "USER".into(),
                    text: format!("[m{i}] text{i}"),
                }]
            })
            .collect();
        let plan = prepare_summary_inputs(&chunks, &Lang::En).unwrap();
        // Echoing is a local stand-in for a model, testing transport rather than semantics.
        let summaries = plan.inputs.iter().map(|(_, input)| input.clone()).collect();
        let result = combine_summaries(&plan, summaries, chunks.len()).unwrap();
        assert!(
            result
                .text
                .contains("Synthetic summary of source chunk 1/5")
        );
        assert!(
            result
                .text
                .contains("Synthetic summary of source chunk 4/5")
        );
        assert!(result.text.ends_with("[USER] [m5] text5\n"));
        assert!(!result.text.contains("[m2]"));
        assert!(
            result
                .text
                .starts_with(result.coverage_notice.as_deref().unwrap())
        );
        assert!(result.text.contains("2..=3"));
        assert!(combine_summaries(&plan, vec!["ok".into()], 5).is_err());
        assert!(combine_summaries(&plan, vec!["ok".into(), " ".into(), "ok".into()], 5).is_err());
        assert!(prepare_summary_inputs(&[], &Lang::Ko).is_err());
    }

    #[test]
    fn test_selected_inputs_include_changed_decision_resolution_and_final_correction() {
        let chunks = vec![
            vec![fragment(
                1,
                "USER",
                "[m1] A를 선택합니다. 빌드가 실패했습니다.",
            )],
            vec![fragment(2, "ASSISTANT", "[m2] 중간 작업")],
            vec![
                fragment(
                    3,
                    "ASSISTANT",
                    "[m3] A로 진행합니다. 테스트는 실패했습니다.",
                ),
                fragment(
                    4,
                    "USER",
                    "[m4] B로 바꿉니다. 설정 수정 후 빌드가 통과했습니다.",
                ),
            ],
            vec![fragment(
                5,
                "USER",
                "[m5] 마지막 정정입니다. 테스트도 모두 통과했습니다.",
            )],
        ];
        let plan = prepare_summary_inputs(&chunks, &Lang::Ko).unwrap();
        assert_eq!(plan.inputs.len(), 3);
        let text = plan
            .inputs
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("[m1] A를 선택합니다"));
        assert!(!text.contains("[m2]"));
        assert!(text.contains("[ASSISTANT] [m3]"));
        assert!(text.contains("[USER] [m4] B로 바꿉니다"));
        assert!(text.contains("[USER] [m5] 마지막 정정"));
        assert!(text.find("[m1]").unwrap() < text.find("[m3]").unwrap());
        assert!(text.find("[m4]").unwrap() < text.find("[m5]").unwrap());
        let notice = plan.coverage_notice.unwrap();
        assert!(notice.contains("2..=2"));
        assert!(notice.contains('4'));
    }

    #[test]
    fn test_large_message_respects_token_limit_including_role() {
        let messages = vec![("USER".into(), "가나다🙂".repeat(100))];
        let chunks = split_into_chunks(&messages, 25, TEST_MAX_CHARS).unwrap();
        assert!(chunks.len() > 1);
        let mut restored = String::new();
        for chunk in chunks {
            let rendered: String = chunk
                .iter()
                .map(|f| format!("[{}] {}\n", f.role, f.text))
                .collect();
            assert!(estimate_tokens(&rendered) <= 25);
            for fragment in chunk {
                restored.push_str(&fragment.text);
            }
        }
        assert_eq!(restored, messages[0].1);
    }

    #[test]
    fn test_split_message_keeps_evidence_id_on_every_fragment() {
        let text = format!("[m42] {}", "앞뒤맥락🙂".repeat(40));
        let chunks = split_into_chunks(&[("USER".into(), text.clone())], 25, 64).unwrap();
        assert!(chunks.len() > 2);
        let mut restored = String::new();
        for chunk in chunks {
            for fragment in chunk {
                assert_eq!(fragment.role, "USER");
                assert_eq!(fragment.message_index, 1);
                restored.push_str(fragment.text.strip_prefix("[m42] ").expect("original ID"));
            }
        }
        assert_eq!(format!("[m42] {restored}"), text);
    }

    #[test]
    fn test_split_into_chunks_respects_token_limit() {
        let messages = vec![
            ("USER".to_string(), "a".repeat(20)),
            ("USER".to_string(), "b".repeat(20)),
            ("USER".to_string(), "c".repeat(20)),
        ];
        let chunks = split_into_chunks(&messages, 30, TEST_MAX_CHARS).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1].len(), 1);
        for chunk in chunks {
            let chars: usize = chunk
                .iter()
                .map(|f| rendered_message_chars(&f.role, &f.text))
                .sum();
            assert!(chars <= 60);
        }
    }

    #[test]
    fn test_split_rejects_budgets_too_small_for_role_or_id() {
        let messages = vec![("USER".into(), "[m123] text".into())];
        for (tokens, chars) in [(0, 100), (100, 0), (100, 7), (100, 15)] {
            assert!(split_into_chunks(&messages, tokens, chars).is_err());
        }
        let chunks = split_into_chunks(&messages, 100, 16).unwrap();
        assert_eq!(chunks.len(), 4);
        assert_eq!(chunks.last().unwrap()[0].text, "[m123] t");
    }

    #[test]
    fn test_split_short_input_keeps_messages_and_order() {
        let messages = vec![
            ("USER".into(), "[m1] A를 사용합니다".into()),
            ("ASSISTANT".into(), "[m2] 알겠습니다".into()),
            ("USER".into(), String::new()),
        ];
        assert_eq!(
            split_into_chunks(&messages, 100, 200).unwrap(),
            vec![
                messages
                    .into_iter()
                    .enumerate()
                    .map(|(i, (role, text))| InputFragment {
                        message_index: i + 1,
                        role,
                        text
                    })
                    .collect::<Chunk>()
            ]
        );
    }

    #[test]
    fn test_split_into_chunks_single_message_exceeds_limit() {
        let messages = vec![("USER".to_string(), "a".repeat(100))];
        let chunks = split_into_chunks(&messages, 25, 32).unwrap();
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn test_split_into_chunks_empty_messages() {
        let messages: Vec<(String, String)> = vec![];
        let chunks = split_into_chunks(&messages, 30_000, TEST_MAX_CHARS).unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_split_into_chunks_respects_char_limit() {
        let messages = vec![("ASSISTANT".to_string(), "x".repeat(200))];
        let chunks = split_into_chunks(&messages, 1_000_000, 64).unwrap();
        assert!(chunks.len() >= 3);
        for chunk in chunks {
            let rendered: usize = chunk
                .iter()
                .map(|f| rendered_message_chars(&f.role, &f.text))
                .sum();
            assert!(rendered <= 64);
        }
    }

    #[test]
    fn test_calculate_wait_itpm_based() {
        let limits = RateLimits {
            input_tokens_per_minute: 30_000,
            output_tokens_per_minute: 8_000,
            requests_per_minute: 1_000,
        };
        let wait = calculate_wait(15_000, &limits);
        assert!((wait - 30.0).abs() < 0.1);
    }

    #[test]
    fn test_calculate_wait_rpm_based() {
        let limits = RateLimits {
            input_tokens_per_minute: 1_000_000,
            output_tokens_per_minute: 200_000,
            requests_per_minute: 50,
        };
        let wait = calculate_wait(100, &limits);
        assert!((wait - 1.2).abs() < 0.1);
    }
}
