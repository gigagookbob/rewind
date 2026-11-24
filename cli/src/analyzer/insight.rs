// Parses LLM JSON responses into structured insight types and merges split results.

use serde::{Deserialize, Serialize};

/// Defensive deserializer: accepts any JSON type and stringifies non-String values.
pub(super) fn string_or_stringify<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    match v {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Null => Ok(String::new()),
        other => Ok(other.to_string()),
    }
}

/// Defensive deserializer for Vec<T> fields that treats JSON `null` as an empty Vec.
pub(super) fn vec_or_default<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}

/// Defensive deserializer for Vec<String> fields.
/// Converts non-String array elements (e.g., objects) to JSON strings and treats `null` as empty.
fn vec_string_or_stringify<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = Option::<Vec<serde_json::Value>>::deserialize(deserializer)?.unwrap_or_default();
    Ok(values
        .into_iter()
        .map(|v| match v {
            serde_json::Value::String(s) => s,
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        })
        .collect())
}

/// Top-level response containing all LLM-extracted insights.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub sessions: Vec<SessionInsight>,
}

/// Per-session insight matching the insight categories defined in ARCHITECTURE.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInsight {
    /// Set from local log metadata, never accepted from model output.
    #[serde(default)]
    pub project: Option<super::retrospective::Project>,
    #[serde(default)]
    pub task_title: Option<super::retrospective::TaskTitle>,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub questions: Vec<super::retrospective::Question>,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub follow_ups: Vec<super::retrospective::FollowUp>,
    pub session_id: String,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub work_summary: String,
    /// User's decision points -- what was chosen and why.
    #[serde(default, deserialize_with = "vec_or_default")]
    pub decisions: Vec<Decision>,
    /// Legacy compatibility field; omitted by newer prompts and ignored in output.
    #[serde(default, deserialize_with = "vec_string_or_stringify")]
    pub curiosities: Vec<String>,
    /// Factual corrections, preference/approach changes, or ambiguous responses.
    #[serde(default, deserialize_with = "vec_or_default")]
    pub corrections: Vec<Correction>,
    /// Concrete learnings from this session (title + context).
    #[serde(default, deserialize_with = "vec_or_default")]
    pub til: Vec<TilItem>,
}

/// Today I Learned item.
/// Extracted directly from the conversation by the LLM, not derived from curiosities/corrections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilItem {
    #[serde(default, deserialize_with = "vec_or_default")]
    pub evidence: Vec<Evidence>,
    /// One-line summary of what was learned.
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub title: String,
    /// 1-2 lines on why it mattered and how it was applied.
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub detail: String,
}

/// A decision point (why option A was chosen over B).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    #[serde(default, deserialize_with = "vec_or_default")]
    pub evidence: Vec<Evidence>,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub what: String,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub why: String,
}

/// A user response changing a claim or approach; kind distinguishes the two.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    #[serde(default)]
    pub kind: CorrectionKind,
    #[serde(default, deserialize_with = "vec_or_default")]
    pub evidence: Vec<Evidence>,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub model_said: String,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub user_corrected: String,
}

/// A quotation proposed by the model. The executor resolves its local location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub is_reference: bool,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub message_id: String,
    #[serde(default, deserialize_with = "string_or_stringify")]
    pub quote: String,
    #[serde(default)]
    pub location: Option<crate::parser::SourceLocation>,
    #[serde(default)]
    pub role: String,
}

/// Legacy and ambiguous corrections must never default to factual errors.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionKind {
    FactualError,
    PreferenceChange,
    #[default]
    #[serde(other)]
    Unknown,
}

impl CorrectionKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::FactualError => "Factual correction / 사실 오류 정정",
            Self::PreferenceChange => "Preference or approach change / 선호 또는 접근 방식 변경",
            Self::Unknown => "Unclassified / 분류 미확인",
        }
    }
}

/// Parses Claude/OpenAI API raw text response into an AnalysisResult.
///
/// The LLM is instructed to return JSON only, but sometimes:
/// - wraps it in markdown code fences (```json...```)
/// - prepends or appends free-form text around the JSON object
///
/// We extract the first complete `{"sessions": ...}` object defensively.
pub fn parse_response(raw_text: &str) -> Result<AnalysisResult, super::AnalyzerError> {
    let cleaned = strip_code_fences(raw_text);

    serde_json::from_str::<AnalysisResult>(&cleaned)
        .or_else(|_| extract_json_object(&cleaned))
        .map_err(|e| {
            crate::messages::error::json_parse_failed(
                parse_error_category(&e),
                e.line(),
                e.column(),
            )
            .into()
        })
}

fn parse_error_category(error: &serde_json::Error) -> &'static str {
    match error.classify() {
        serde_json::error::Category::Syntax => "syntax",
        serde_json::error::Category::Eof => "truncated",
        serde_json::error::Category::Data => "schema",
        serde_json::error::Category::Io => "io",
    }
}

/// Parses one JSON object from the start of `slice`, ignoring trailing text.
fn parse_prefix_object(slice: &str) -> Result<AnalysisResult, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(slice);
    AnalysisResult::deserialize(&mut deserializer)
}

/// Extracts a JSON object from text that may contain non-JSON content.
///
/// Tries every `{` whose first key is `"sessions"` (supporting pretty-printed
/// JSON and trailing prose or closing code fences), then falls back to the first `{`.
fn extract_json_object(text: &str) -> Result<AnalysisResult, serde_json::Error> {
    let mut first_sessions_err = None;
    for (start, _) in text.match_indices('{') {
        if !text[start + 1..].trim_start().starts_with("\"sessions\"") {
            continue;
        }
        match parse_prefix_object(&text[start..]) {
            Ok(result) => return Ok(result),
            Err(err) => {
                first_sessions_err.get_or_insert(err);
            }
        }
    }
    if let Some(err) = first_sessions_err {
        return Err(err);
    }
    if let Some(start) = text.find('{') {
        return parse_prefix_object(&text[start..]);
    }
    serde_json::from_str::<AnalysisResult>(text)
}

/// Merges multiple AnalysisResults into one by concatenating their session vecs.
/// Used to combine per-session fallback results into a single output.
pub fn merge_results(results: Vec<AnalysisResult>) -> AnalysisResult {
    let sessions = results.into_iter().flat_map(|r| r.sessions).collect();
    AnalysisResult { sessions }
}

fn strip_code_fences(text: &str) -> String {
    let trimmed = text.trim();

    if let Some(after_prefix) = trimmed.strip_prefix("```json")
        && let Some(content) = after_prefix.strip_suffix("```")
    {
        return content.trim().to_string();
    }
    if let Some(after_prefix) = trimmed.strip_prefix("```")
        && let Some(content) = after_prefix.strip_suffix("```")
    {
        return content.trim().to_string();
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response_valid_json_returns_analysis_result() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"파서 모듈 구현","decisions":[{"what":"serde 사용","why":"자동 역직렬화가 편리"}],"curiosities":["serde의 tag 속성은 무엇인가?"],"corrections":[]}]}"#;
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.sessions[0].decisions.len(), 1);
        assert_eq!(result.sessions[0].curiosities.len(), 1);
        assert_eq!(result.sessions[0].corrections.len(), 0);
    }

    #[test]
    fn test_parse_response_strips_code_fences_and_parses() {
        let json = "```json\n{\"sessions\":[]}\n```";
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions.len(), 0);
    }

    #[test]
    fn test_parse_response_invalid_json_returns_error() {
        let result = parse_response("이것은 JSON이 아닙니다");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_til_field_present() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[],"curiosities":[],"corrections":[],"til":[{"title":"serde tag 한계","detail":"중첩 JSON에서 안 먹힌다"}]}]}"#;
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions[0].til.len(), 1);
        assert_eq!(result.sessions[0].til[0].title, "serde tag 한계");
    }

    #[test]
    fn test_merge_results_multiple() {
        let r1 = AnalysisResult {
            sessions: vec![SessionInsight {
                project: None,
                task_title: None,
                questions: vec![],
                follow_ups: vec![],
                session_id: "s1".to_string(),
                work_summary: "작업1".to_string(),
                decisions: vec![],
                curiosities: vec![],
                corrections: vec![],
                til: vec![],
            }],
        };
        let r2 = AnalysisResult {
            sessions: vec![SessionInsight {
                project: None,
                task_title: None,
                questions: vec![],
                follow_ups: vec![],
                session_id: "s2".to_string(),
                work_summary: "작업2".to_string(),
                decisions: vec![],
                curiosities: vec![],
                corrections: vec![],
                til: vec![],
            }],
        };
        let merged = merge_results(vec![r1, r2]);
        assert_eq!(merged.sessions.len(), 2);
        assert_eq!(merged.sessions[0].session_id, "s1");
        assert_eq!(merged.sessions[1].session_id, "s2");
    }

    #[test]
    fn test_merge_results_empty_vec_empty_result() {
        let merged = merge_results(vec![]);
        assert!(merged.sessions.is_empty());
    }

    #[test]
    fn test_parse_response_til_defaults_to_empty() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[],"curiosities":[],"corrections":[]}]}"#;
        let result = parse_response(json).unwrap();
        assert!(result.sessions[0].til.is_empty());
    }

    // --- LLM response field-missing tolerance tests ---

    #[test]
    fn test_til_missing_title_parses_ok() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[],"curiosities":[],"corrections":[],"til":[{"detail":"이유 설명"}]}]}"#;
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions[0].til.len(), 1);
        assert_eq!(result.sessions[0].til[0].title, "");
        assert_eq!(result.sessions[0].til[0].detail, "이유 설명");
    }

    #[test]
    fn test_til_missing_detail_parses_ok() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[],"curiosities":[],"corrections":[],"til":[{"title":"배운 점"}]}]}"#;
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions[0].til[0].detail, "");
    }

    #[test]
    fn test_decision_missing_field_parses_ok() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[{"what":"serde 선택"}],"curiosities":[],"corrections":[],"til":[]}]}"#;
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions[0].decisions[0].what, "serde 선택");
        assert_eq!(result.sessions[0].decisions[0].why, "");
    }

    #[test]
    fn test_correction_missing_field_parses_ok() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[],"curiosities":[],"corrections":[{"model_said":"잘못된 설명"}],"til":[]}]}"#;
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions[0].corrections[0].model_said, "잘못된 설명");
        assert_eq!(result.sessions[0].corrections[0].user_corrected, "");
    }

    // --- LLM response type-mismatch tolerance tests (map -> string defense) ---

    #[test]
    fn test_curiosities_object_array_stringify() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[],"curiosities":[{"question":"Xcode 호환성","context":"빌드 실패"}],"corrections":[]}]}"#;
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions[0].curiosities.len(), 1);
        assert!(result.sessions[0].curiosities[0].contains("Xcode 호환성"));
    }

    #[test]
    fn test_work_summary_object_stringify() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":{"main":"요약","detail":"상세"},"decisions":[],"curiosities":[],"corrections":[]}]}"#;
        let result = parse_response(json).unwrap();
        assert!(result.sessions[0].work_summary.contains("요약"));
    }

    #[test]
    fn test_decision_why_object_stringify() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":"요약","decisions":[{"what":"선택","why":{"reason":"이유","context":"맥락"}}],"curiosities":[],"corrections":[]}]}"#;
        let result = parse_response(json).unwrap();
        assert!(result.sessions[0].decisions[0].why.contains("이유"));
    }

    #[test]
    fn test_parse_response_with_preamble_text() {
        let raw = r#"Looking at this conversation, here is the analysis:

{"sessions":[{"session_id":"s1","work_summary":"작업 요약","decisions":[],"curiosities":[],"corrections":[]}]}"#;
        let result = parse_response(raw).unwrap();
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.sessions[0].session_id, "s1");
    }

    #[test]
    fn test_parse_response_missing_curiosities_and_optional_arrays_parses_ok() {
        let json = r#"{"sessions":[{"session_id":"d109d229","work_summary":"다이어그램 생성","task_title":{"text":"다이어그램 생성","evidence":[{"message_id":"m1","quote":"Use skills"}]},"questions":[],"follow_ups":[],"decisions":[],"corrections":[],"til":[]}]}"#;
        let result = parse_response(json).unwrap();
        assert_eq!(result.sessions[0].session_id, "d109d229");
        assert!(result.sessions[0].curiosities.is_empty());

        let minimal = r#"{"sessions":[{"session_id":"s2"}]}"#;
        let minimal_result = parse_response(minimal).unwrap();
        assert_eq!(minimal_result.sessions[0].session_id, "s2");
        assert_eq!(minimal_result.sessions[0].work_summary, "");
        assert!(minimal_result.sessions[0].decisions.is_empty());
        assert!(minimal_result.sessions[0].corrections.is_empty());
        assert!(minimal_result.sessions[0].curiosities.is_empty());
    }

    #[test]
    fn test_parse_response_null_collection_fields_default_to_empty() {
        let json = r#"{"sessions":[{"session_id":"s1","work_summary":null,"decisions":null,"curiosities":null,"corrections":null,"til":null,"questions":[{"question":"질문","evidence":null,"updates":null}],"follow_ups":null}]}"#;
        let result = parse_response(json).unwrap();
        let s = &result.sessions[0];
        assert_eq!(s.work_summary, "");
        assert!(s.decisions.is_empty());
        assert!(s.curiosities.is_empty());
        assert!(s.corrections.is_empty());
        assert!(s.til.is_empty());
        assert!(s.follow_ups.is_empty());
        assert_eq!(s.questions.len(), 1);
        assert!(s.questions[0].evidence.is_empty());
        assert!(s.questions[0].updates.is_empty());
    }

    #[test]
    fn test_parse_response_with_preamble_braces_code_fence_and_trailing_text() {
        let raw = r#"참고 {예시} 분석 결과입니다:
```json
{
  "sessions": [
    {
      "session_id": "s1",
      "work_summary": "작업 요약",
      "decisions": [],
      "corrections": []
    }
  ]
}
```
추가 설명입니다."#;
        let result = parse_response(raw).unwrap();
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(result.sessions[0].session_id, "s1");
    }

    #[test]
    fn test_parse_response_error_reports_safe_category_without_body() {
        let syntax_err = parse_response("private text {invalid}")
            .unwrap_err()
            .to_string();
        assert!(syntax_err.starts_with(crate::messages::error::JSON_PARSE_FAILED_MARKER));
        assert!(syntax_err.contains("(syntax)"), "{syntax_err}");
        assert!(!syntax_err.contains("private text"));

        let eof_err = parse_response(r#"{"sessions":[{"session_id":"secret-id""#)
            .unwrap_err()
            .to_string();
        assert!(eof_err.contains("(truncated)"), "{eof_err}");
        assert!(!eof_err.contains("secret-id"));

        let schema_err = parse_response(r#"{"sessions":[{"work_summary":"secret-summary"}]}"#)
            .unwrap_err()
            .to_string();
        assert!(schema_err.contains("(schema)"), "{schema_err}");
        assert!(!schema_err.contains("secret-summary"));
    }
}
