//! Both renderers show the same read-only timeline and conservative labels.
use crate::analyzer::insight::SessionInsight;

pub(crate) const NOTE: &str = "Conversation state only; put manual notes outside the generated region. / 대화에서 확인한 상태입니다. 수동 메모는 자동 생성 영역 밖에 씁니다.";

pub(crate) fn title(session: &SessionInsight) -> &str {
    session
        .task_title
        .as_ref()
        .map_or("Task title unknown / 작업 제목 미확인", |t| {
            t.text.as_str()
        })
}

pub(crate) fn lines(session: &SessionInsight) -> Vec<String> {
    let mut lines = Vec::new();
    let proofs = |lines: &mut Vec<String>, evidence: &[_]| {
        lines.extend(
            super::display_evidence(evidence)
                .lines()
                .map(str::to_string),
        );
    };
    if let Some(title) = &session.task_title {
        proofs(&mut lines, &title.evidence);
    }
    if !session.questions.is_empty() {
        lines.push("Questions / 질문".into());
    }
    for question in &session.questions {
        lines.push(format!(
            "{}: {}",
            question.status_label(),
            question.question
        ));
        proofs(&mut lines, &question.evidence);
        for update in &question.updates {
            lines.push(format!("  {}: {}", update.kind.label(), update.detail));
            proofs(&mut lines, &update.evidence);
        }
    }
    for curiosity in &session.curiosities {
        lines.push(format!("Legacy question; state and evidence unknown / 기존 질문, 상태와 근거 미확인: {curiosity}"));
    }
    if !session.follow_ups.is_empty() {
        lines.push("Follow-up work / 후속 작업".into());
    }
    for task in &session.follow_ups {
        lines.push(format!(
            "{}: {} ({})",
            task.status_label(),
            task.task,
            task.origin.label()
        ));
        proofs(&mut lines, &task.evidence);
        for update in &task.updates {
            lines.push(format!("  {}: {}", update.kind.label(), update.detail));
            proofs(&mut lines, &update.evidence);
        }
    }
    lines.into_iter().map(|line| super::plain(&line)).collect()
}
