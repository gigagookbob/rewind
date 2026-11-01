//! Only unchanged, explicitly owned regions may be replaced.

use super::OutputError;
use std::ops::Range;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Section {
    Analysis,
    Summary,
}

impl Section {
    fn name(self) -> &'static str {
        match self {
            Self::Analysis => "analysis",
            Self::Summary => "summary",
        }
    }
}

fn checksum(body: &str) -> String {
    // Fixed FNV-1a, so upgrading Rust cannot change the stored checksum.
    let hash = body.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

fn conflict() -> OutputError {
    "일일 노트의 rwd 영역이 수정되었거나 경계가 불분명해 저장하지 않았습니다. 원본을 보존했습니다. 메모를 영역 밖으로 옮기고 원래 본문을 복원하거나, 시작과 끝 표시만 모두 제거해 기존 내용을 수동 기록으로 보존한 뒤 다시 실행하세요.".into()
}

/// Fenced examples are never ownership declarations. Recognize longer fences
/// and both fence characters, including indented examples conservatively.
fn fence_line(line: &str, fence: &mut Option<(u8, usize)>) -> bool {
    let text = line.trim_start();
    let Some(first) = text.bytes().next() else {
        return fence.is_some();
    };
    let count = text.bytes().take_while(|&c| c == first).count();
    if let Some((kind, width)) = *fence {
        if first == kind && count >= width && text[count..].trim().is_empty() {
            *fence = None;
        }
        return true;
    }
    if (first == b'`' || first == b'~') && count >= 3 {
        if first != b'`' || !text[count..].contains('`') {
            *fence = Some((first, count));
        }
        return fence.is_some();
    }
    false
}

pub(super) fn update(existing: &str, section: Section, body: &str) -> Result<String, OutputError> {
    let ranges = regions(existing)?;
    let body = format!("{}\n", body.trim_end_matches('\n'));
    let block = format!(
        "<!-- rwd:{}:v1:{} -->\n{body}<!-- /rwd:{} -->\n",
        section.name(),
        checksum(&body),
        section.name()
    );
    let updated = if let Some((_, range)) = ranges.iter().find(|(kind, _)| *kind == section) {
        format!(
            "{}{}{}",
            &existing[..range.start],
            block,
            &existing[range.end..]
        )
    } else {
        let separator = if existing.is_empty() || existing.ends_with("\n\n") {
            ""
        } else if existing.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        format!("{existing}{separator}{block}")
    };
    // Unclosed fences or injected delimiters in generated text must not make
    // the next run mistake existing user text for generated content.
    let checked = regions(&updated)?;
    if checked.iter().filter(|(kind, _)| *kind == section).count() != 1 {
        return Err(conflict());
    }
    Ok(updated)
}

fn regions(text: &str) -> Result<Vec<(Section, Range<usize>)>, OutputError> {
    let mut found = Vec::new();
    let mut opened: Option<(Section, usize, usize, String)> = None;
    let mut fence = None;
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches(['\r', '\n']);
        if fence_line(line, &mut fence) {
            continue;
        }
        if !line.contains("<!-- rwd:") && !line.contains("<!-- /rwd:") {
            continue;
        }
        let mut recognized = false;
        for section in [Section::Analysis, Section::Summary] {
            let prefix = format!("<!-- rwd:{}:v1:", section.name());
            if let Some(hash) = line
                .strip_prefix(&prefix)
                .and_then(|s| s.strip_suffix(" -->"))
            {
                if opened.is_some()
                    || found.iter().any(|(kind, _)| *kind == section)
                    || hash.len() != 16
                    || !hash.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err(conflict());
                }
                opened = Some((section, start, offset, hash.to_string()));
                recognized = true;
                break;
            }
            if line == format!("<!-- /rwd:{} -->", section.name()) {
                let (kind, begin, body_start, hash) = opened.take().ok_or_else(conflict)?;
                if kind != section || checksum(&text[body_start..start]) != hash {
                    return Err(conflict());
                }
                found.push((kind, begin..offset));
                recognized = true;
                break;
            }
        }
        if !recognized {
            return Err(conflict());
        }
    }
    if opened.is_some() || fence.is_some() {
        return Err(conflict());
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_preserves_notes_and_independent_regions() {
        let note = "---\ntitle: 오늘\n---\n# 수동 제목\n\n## Development Progress\n기존 진행\n";
        for first in [Section::Analysis, Section::Summary] {
            let second = if first == Section::Analysis {
                Section::Summary
            } else {
                Section::Analysis
            };
            let once = update(note, first, "first").unwrap();
            let both = update(&once, second, "second").unwrap();
            let again = update(&both, first, "updated").unwrap();
            assert!(again.starts_with(note));
            assert!(again.contains("second\n"));
            assert!(!again.contains("first\n"));
            assert_eq!(update(&again, first, "updated").unwrap(), again);
        }
    }

    #[test]
    fn test_modified_or_ambiguous_regions_are_not_overwritten() {
        let block = update("", Section::Analysis, "generated").unwrap();
        let summary = update("", Section::Summary, "progress").unwrap();
        let cases = [
            block.replace("generated\n", "generated\nmanual memo\n"),
            block.replace("v1:", "v2:"),
            block.replace("<!-- /rwd:analysis -->\n", ""),
            block.replace("<!-- rwd:", "<!-- broken:"),
            block.replace("/rwd:analysis", "/rwd:summary"),
            format!("{block}{block}"),
            format!("<!-- /rwd:analysis -->\n{block}"),
            format!("{block}<!-- rwd:summary:v1:bad -->\n"),
            format!("{block}{}", summary.replace("progress", "manual")),
            format!("{block}{}", summary.replace("progress", &block)),
        ];
        for existing in cases {
            assert!(
                update(&existing, Section::Analysis, "new").is_err(),
                "{existing}"
            );
        }
    }

    #[test]
    fn test_fenced_titles_and_markers_are_examples() {
        let example = update("", Section::Summary, "example").unwrap();
        for (open, close) in [
            ("```markdown", "```"),
            ("~~~~", "~~~~"),
            ("````md", "````"),
            ("    ```", "    ```"),
        ] {
            let note = format!("{open}\n## Development Progress\n{example}```\n{close}\n");
            // For a triple-backtick opening, the inner ``` closes the fence.
            let note = if open.trim_start().starts_with("```") && !open.starts_with("````") {
                format!("{open}\n## Development Progress\n{example}{close}\n")
            } else {
                note
            };
            let once = update(&note, Section::Summary, "real").unwrap();
            let twice = update(&once, Section::Summary, "updated").unwrap();
            assert!(twice.starts_with(&note));
            assert!(twice.contains("example\n"));
            assert!(!twice.contains("real\n"));
            assert_eq!(update(&twice, Section::Summary, "updated").unwrap(), twice);
        }
    }

    #[test]
    fn test_unclosed_fences_and_generated_markers_fail_safely() {
        for note in ["```md\nuser text", "~~~\nuser text"] {
            assert!(update(note, Section::Analysis, "new").is_err());
        }
        for body in [
            "```\nunclosed",
            "<!-- /rwd:analysis -->",
            "<!-- rwd:unknown -->",
        ] {
            assert!(update("user text", Section::Analysis, body).is_err());
        }
    }

    #[test]
    fn test_plain_rwd_text_is_not_an_ownership_marker() {
        let note = "---\ntopic: rwd:daily\n---\n# rwd: 실행 메모\n내용과 `rwd:summary` 예시\n";
        let once = update(note, Section::Analysis, "rwd: ordinary generated text").unwrap();
        let twice = update(&once, Section::Analysis, "new").unwrap();
        assert!(twice.starts_with(note));
        assert_eq!(update(&twice, Section::Analysis, "new").unwrap(), twice);
    }
}
