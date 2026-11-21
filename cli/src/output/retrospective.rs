//! Titles and questions shared by the terminal and Markdown renderers.
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

/// Answered questions are left out: they read as noise the next day.
/// Legacy `curiosities` carry no state, so they are not shown either.
pub(crate) fn unresolved_questions(
    session: &SessionInsight,
) -> impl Iterator<Item = &crate::analyzer::retrospective::Question> {
    session.questions.iter().filter(|q| q.is_unresolved())
}
