// Large session chunking and summarization module.
//
// Splits sessions exceeding ITPM into message-level chunks,
// summarizes each chunk via the LLM, then combines results.

use super::planner::RateLimits;
use super::prompt::estimate_tokens;

use crate::config::Lang;

/// Safety cap on the number of chunk-summarize API calls we make for a single
/// session. A pathological session that still explodes into dozens of chunks
/// after compaction would otherwise burn through the provider's daily quota.
pub const MAX_CHUNKS_PER_SESSION: usize = 3;

const CHUNK_SUMMARIZE_PROMPT_EN: &str = include_str!("../../prompts/chunk_summarize_en.md");
const CHUNK_SUMMARIZE_PROMPT_KO: &str = include_str!("../../prompts/chunk_summarize_ko.md");

pub type Chunk = Vec<(String, String)>;

pub fn get_chunk_summarize_prompt(lang: &Lang) -> &'static str {
    match lang {
        Lang::En => CHUNK_SUMMARIZE_PROMPT_EN,
        Lang::Ko => CHUNK_SUMMARIZE_PROMPT_KO,
    }
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

    for (role, text) in messages {
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

            current_chunk.push((role.clone(), part));
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
    let itpm_wait = (used_tokens as f64 / limits.input_tokens_per_minute as f64) * 60.0;
    let rpm_wait = 60.0 / limits.requests_per_minute as f64;
    itpm_wait.max(rpm_wait)
}

/// Summarizes a large session's chunks and returns the combined summary text.
/// Applies rate pacing between chunk API calls.
pub async fn summarize_chunks(
    chunks: &[Vec<(String, String)>],
    provider: &super::provider::LlmProvider,
    api_key: &str,
    limits: &RateLimits,
    lang: &Lang,
) -> Result<String, super::AnalyzerError> {
    let effective_chunks: &[Vec<(String, String)>] = if chunks.len() > MAX_CHUNKS_PER_SESSION {
        eprintln!(
            "{}",
            crate::messages::status::chunk_cap_applied(chunks.len(), MAX_CHUNKS_PER_SESSION)
        );
        &chunks[..MAX_CHUNKS_PER_SESSION]
    } else {
        chunks
    };

    let mut summaries: Vec<String> = Vec::new();
    let total = effective_chunks.len();

    for (i, chunk) in effective_chunks.iter().enumerate() {
        let chunk_text: String = chunk
            .iter()
            .map(|(role, text)| format!("[{role}] {text}"))
            .collect::<Vec<_>>()
            .join("\n");

        let sp = super::start_spinner(crate::messages::status::chunk_summarizing(i + 1, total));

        let (summary, _usage) = provider
            .call_text_api(api_key, get_chunk_summarize_prompt(lang), &chunk_text, 2000)
            .await?;
        super::stop_spinner(sp);
        eprintln!("{}", crate::messages::status::chunk_done(i + 1, total));
        summaries.push(summary);

        // Rate pacing: countdown wait unless this is the last chunk.
        if i + 1 < total {
            let chunk_tokens = estimate_tokens(&chunk_text);
            let wait = calculate_wait(chunk_tokens, limits);
            if wait > 0.0 {
                super::countdown_sleep(wait.ceil() as u64).await;
            }
        }
    }

    Ok(summaries.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEST_MAX_CHARS: usize = 1_048_576;

    #[test]
    fn test_large_message_respects_token_limit_including_role() {
        let messages = vec![("USER".into(), "가나다🙂".repeat(100))];
        let chunks = split_into_chunks(&messages, 25, TEST_MAX_CHARS).unwrap();
        assert!(chunks.len() > 1);
        let mut restored = String::new();
        for chunk in chunks {
            let rendered: String = chunk.iter().map(|(r, t)| format!("[{r}] {t}\n")).collect();
            assert!(estimate_tokens(&rendered) <= 25);
            for (_, text) in chunk {
                restored.push_str(&text);
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
            for (role, part) in chunk {
                assert_eq!(role, "USER");
                restored.push_str(part.strip_prefix("[m42] ").expect("original ID"));
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
                .map(|(r, t)| rendered_message_chars(r, t))
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
        assert_eq!(chunks.last().unwrap()[0].1, "[m123] t");
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
            vec![messages]
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
                .map(|(role, text)| rendered_message_chars(role, text))
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
