# 여러 AI 에이전트 로그를 읽는 어댑터 구조

작성일: 2026-10-24
상태: 승인됨(2026-10-25). 구현 계획은 [tasks/plan.md](tasks/plan.md)에 있습니다.

첫 초안은 Gemini CLI를 첫 대상으로 잡았습니다. Gemini CLI는 무료 등급과 Google One 사용자에게는 2026-06-18에 Antigravity CLI(`agy`)로 바뀌었고([공식 README](https://github.com/google-gemini/gemini-cli)), 이 컴퓨터에서도 실제로 쓰는 도구는 agy와 Antigravity 앱입니다. 그래서 첫 대상을 Antigravity로 바꿨습니다.

## 목표

rwd는 지금 Claude Code와 Codex 로그만 읽습니다. 두 출처가 `run_today`, 캐시, 터미널 정보 상자, `rwd doctor`에 직접 박혀 있어서, 에이전트를 하나 더 붙이려면 이 네 곳을 모두 고쳐야 합니다.

이번 작업은 에이전트마다 로그를 읽는 일을 어댑터 하나에 모으고, 나머지 코드는 어댑터 목록을 돌기만 하도록 바꿉니다. 그 위에 LLM을 부르지 않고 찾은 세션만 보여 주는 확인 옵션을 넣고, 첫 새 어댑터로 Antigravity를 붙입니다.

사용자 입장에서 달라지는 점은 두 가지입니다.

- agy, Antigravity 앱, Antigravity IDE에서 한 대화도 `rwd today` 결과와 Obsidian 파일에 `Antigravity` 절로 들어갑니다. `agy -p`처럼 한 번 묻고 끝나는 실행은 기본으로 뺍니다.
- `rwd today --dry-run`을 실행하면 출처별로 찾은 세션과 뺀 세션 수만 보여 주고 끝납니다. LLM을 부르지 않아서 비용이 들지 않고 대화도 밖으로 나가지 않습니다.

Claude Code와 Codex 결과는 지금과 같아야 합니다.

작업은 네 단계로 나눕니다. 각 단계가 끝난 상태로도 배포할 수 있어야 합니다.

1. 1단계, 구조 정리: 어댑터 구조를 만들고 Claude Code와 Codex를 그 구조로 옮깁니다. 동작은 바뀌지 않습니다.
2. 2단계, 확인 옵션: `rwd today --dry-run`을 추가합니다. 3단계의 Antigravity 어댑터를 실제 로그로 비용 없이 확인하는 데 씁니다.
3. 3단계, Antigravity 어댑터: 1단계 구조 위에 Antigravity 어댑터를 추가합니다.
4. 4단계, 서브에이전트 결과 합치기: 서브에이전트를 따로 세지는 않되, 무슨 일을 맡았고 무엇을 돌려줬는지는 부모 세션 프롬프트에 붙입니다. 지금은 Claude와 Codex 모두 서브에이전트 내용을 통째로 버립니다.

## 결정된 사항

2026-10-25에 사용자가 정한 내용입니다.

- Antigravity IDE 폴더(`~/.gemini/antigravity-ide`)도 읽습니다.
- `agy -p` 실행을 한 번 해서 `-p` 규칙을 확인했습니다. 결과는 아래 `-p` 실행 빼기 절에 있습니다.
- 서브에이전트는 세션 수에 넣지 않지만 내용은 부모 세션에 남깁니다. 4단계로 따로 둡니다.
- rwd는 업데이트한 날부터 Antigravity 대화를 기본으로 읽습니다. Claude Code, Codex와 같은 방식입니다. 대신 3단계를 담은 릴리스의 릴리스 노트에 이 변경과 끄는 방법을 알립니다(설정 절의 기본값과 릴리스 노트). 이 설정은 rwd 설정이고, Antigravity 쪽 설정은 건드리지 않습니다.
- Antigravity는 대화마다 있는 `brain/<대화 ID>/.system_generated/logs/transcript_full.jsonl`을 주 입력으로 읽습니다. SQLite 파일과 protobuf 해석은 쓰지 않습니다.
- 서브에이전트 최종 결과는 하나당 8,000자, 세션 합계 24,000자까지 넣습니다.
- Codex 서브에이전트의 부모 연결(`parent_thread_id`)은 확인된 규칙으로 씁니다.
- Claude 서브에이전트는 부모 세션 폴더의 `subagents/` 아래 기록 파일을 먼저 읽고, 없을 때만 부모 기록의 도구 결과를 씁니다.
- Antigravity 서브에이전트 표본 없이 진행합니다. 3단계는 서브에이전트를 거르지 않고, 4단계에서 Antigravity는 뺍니다.

## 가정

아래 가정이 틀리면 승인 전에 알려 주세요.

1. Antigravity의 저장 형식은 공개 문서가 없습니다. 이 명세의 형식 설명은 이 컴퓨터의 실제 대화 40개(agy 26개, Antigravity 앱 4개, Antigravity IDE 10개, agy 1.3.2)를 읽기 전용으로 분석한 결과입니다. 2026-10-24에 SQLite 파일을, 2026-10-25에 `transcript_full.jsonl`을 분석하고 두 기록을 서로 대조했습니다. Antigravity가 업데이트되면 바뀔 수 있습니다.
2. 예전 형식 캐시는 한 번 다시 분석합니다. 캐시에 시간대 정보를 넣었을 때도 같은 방식을 썼습니다([cache.rs:57](src/cache.rs)).
3. 새 크레이트는 추가하지 않습니다. Antigravity 기록도 JSONL이라 이미 쓰는 `serde_json`으로 읽습니다.
4. 분석 단계(프롬프트 전송, LLM 호출, 응답 파싱, 비밀값 가리기)는 바꾸지 않습니다.

## 현재 구조에서 출처가 고정된 곳

2026-10-25에 커밋된 명령별 분리(`07e0853`), 설정 분리(`bd7ae47`), 분석 실행 분리(`339ece4`) 뒤의 위치입니다.

| 위치 | 고정된 내용 |
|---|---|
| [commands/today.rs:14](src/commands/today.rs) `run_today` | Claude와 Codex 수집 함수를 직접 부르고(36, 39줄), 출처별 개수로 캐시를 비교하고(113줄), 분석도 출처별로 따로 부릅니다(158, 178줄). 캐시에 저장하는 칸도 출처별입니다(215줄) |
| [commands/collection.rs:45](src/commands/collection.rs), [collection.rs:112](src/commands/collection.rs) | Claude 수집 함수 `collect_claude_entries_with_stats`와 Codex 수집 함수 `collect_codex_sessions`가 한 파일에 있습니다 |
| [commands/summary.rs:106](src/commands/summary.rs) `run_slack` | 캐시가 낡았는지 보려고 두 출처를 다시 수집하고 출처별 칸과 비교합니다(123~132줄) |
| [commands/display.rs:130](src/commands/display.rs) `print_info_box` | Claude와 Codex 칸을 따로 그리고, 활동 시각을 구하는 함수도 출처별로 따로 있습니다(25~87줄) |
| [cache.rs:22](src/cache.rs) `TodayCache` | `claude_entry_count`, `codex_session_count`, `codex_entry_count` 칸이 고정돼 있습니다 |
| [doctor.rs:197](src/doctor.rs) | 두 출처의 로그 폴더만 확인합니다 |
| [analyzer/mod.rs:36](src/analyzer/mod.rs) `SessionSource` | `Claude`, `Codex` 두 값만 있는 enum입니다 |
| [config.rs:163](src/config.rs) `InputConfig` | 출처별 경로 설정이 칸마다 따로 있습니다 |

`commands/`와 `doctor.rs`의 테스트가 아닌 코드에서 `parser::claude`, `parser::codex` 같은 출처별 파서를 직접 부르는 줄은 `collection.rs` 26줄, `display.rs` 12줄, `today.rs` 1줄, `doctor.rs` 2줄입니다.

분석 단계는 이미 출처와 상관없습니다. `analyze_session_tasks`는 세션 ID, 프롬프트 글, (역할, 내용) 목록, 토큰 추정치를 담은 `SessionTask` 목록만 받습니다. 어댑터는 이 `SessionTask`를 만드는 데까지 맡습니다.

## 설계

### 어댑터 계약

```rust
/// One agent's log source. Adapters only read files; they never write.
pub trait SourceAdapter {
    /// Stable id used in config and cache keys, e.g. "antigravity".
    fn id(&self) -> &'static str;
    /// Heading shown in the terminal and Markdown, e.g. "Antigravity".
    fn display_name(&self) -> &'static str;
    /// Collects sessions with activity inside the local date window.
    fn collect(&self, ctx: &CollectContext) -> Result<SourceCollection, ParseError>;
}

pub struct CollectContext<'a> {
    pub date: NaiveDate,
    pub config: Option<&'a Config>,
    pub include_automated: bool,
}

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

pub struct CollectedSession {
    pub task: SessionTask,
    pub first_activity: Option<DateTime<Utc>>,
    pub last_activity: Option<DateTime<Utc>>,
    /// (input, output) totals when the source records token usage.
    pub token_usage: Option<(u64, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFingerprint {
    pub session_count: usize,
    pub entry_count: usize,
}
```

어댑터 목록은 한 함수가 정해진 순서로 돌려줍니다. 순서는 Claude Code, Codex, Antigravity입니다. 터미널과 Markdown의 절 순서도 이 순서를 따릅니다.

```rust
pub fn registered_adapters() -> Vec<Box<dyn SourceAdapter>> {
    vec![
        Box::new(claude::ClaudeAdapter),
        Box::new(codex::CodexAdapter),
        Box::new(antigravity::AntigravityAdapter),
    ]
}
```

trait와 enum 중에서 trait를 고릅니다. `LlmProvider`처럼 enum으로 만들면 출처를 추가할 때 match 문을 여러 곳에서 고쳐야 해서, 이번 작업이 없애려는 결합이 그대로 남습니다. LLM 제공자는 종류가 거의 늘지 않고 호출마다 동작이 달라서 enum이 맞지만, 로그 출처는 계속 늘어날 목록입니다. trait를 쓰면 출처별 타입(`LogEntry`, `CodexEntry`)도 각 어댑터 안에 숨길 수 있습니다.

### 공통 흐름

`commands/today.rs`의 `run_today`는 아래 순서로 바뀝니다. `commands/collection.rs`에는 켜진 어댑터를 차례로 불러 결과를 모으는 함수 하나만 남깁니다.

1. 켜져 있는 어댑터마다 `collect`를 부릅니다. 하나가 `Err`를 돌려주면 그 출처 이름과 오류를 경고로 찍고 나머지 출처를 계속 처리합니다.
2. `--dry-run`이면 확인 화면을 출력하고 끝냅니다(아래 확인 옵션 절).
3. 모든 출처의 세션이 0개면 지금처럼 기록이 없다고 알리고 끝냅니다.
4. 출처별 `SourceFingerprint`를 캐시와 비교합니다. 모두 같고 시간대도 같으면 캐시를 씁니다.
5. 세션이 있는 출처마다 `analyze_session_tasks`를 부르고, 결과를 `(display_name, AnalysisResult)`로 모읍니다.
6. 출력, Markdown 저장, 캐시 저장은 지금과 같습니다.

분석 중 오류가 나면 지금처럼 실행 전체가 실패합니다. 분석 실패를 출처별로 나누는 것은 이번 범위가 아닙니다.

`commands/summary.rs`의 `run_slack`이 하는 낡은 캐시 확인과 `doctor.rs`의 로그 폴더 확인도 같은 어댑터 목록을 씁니다.

### 캐시

`TodayCache`의 출처별 칸 세 개를 출처 id로 찾는 표 하나로 바꿉니다.

```rust
pub struct TodayCache {
    pub date: String,
    #[serde(default)]
    pub source_fingerprints: BTreeMap<String, SourceFingerprint>,
    #[serde(default)]
    pub timezone_window_key: Option<String>,
    pub sources: Vec<(String, AnalysisResult)>,
}
```

캐시를 쓰는 조건은 지금 켜진 어댑터의 지문 표와 캐시의 표가 같고 시간대 키도 같을 때입니다. 세션이 0개인 출처는 표에 넣지 않습니다. 그래서 Antigravity 기록이 없는 날은 어댑터가 생겨도 캐시가 깨지지 않습니다.

`source_fingerprints`가 없는 예전 캐시는 지문이 맞지 않는 것으로 보고 한 번 다시 분석합니다. 예전 칸을 새 표로 옮기는 방법도 있지만, 예전 캐시에는 Claude 세션 수가 없어서 정확히 옮길 수 없습니다. 다시 분석하는 비용은 해당 날짜 한 번입니다.

`rwd summary`와 `rwd slack`이 쓰는 `sources` 부분은 형식이 같아서 예전 캐시도 그대로 읽힙니다.

### 설정

기존 설정 키는 모두 그대로 읽습니다(`input.claude_roots`, `input.codex_roots`, `input.claude.roots`, `input.claude.include_automated`).

새 어댑터는 `input.<출처 id>` 아래에 설정을 둡니다.

```toml
[input.antigravity]
enabled = true              # 기본값 true
roots = []                  # 기본 경로 앞에 추가로 찾을 brain 폴더
include_print_mode = false  # agy -p 실행도 넣을지, 기본값 false
```

`rwd config` 명령과 대화형 메뉴에 새 키를 넣는 것은 이번 범위가 아닙니다.

#### 기본값과 릴리스 노트

`input.antigravity.enabled`의 기본값은 `true`입니다. 설정 파일에 `[input.antigravity]`가 없어도 Antigravity 폴더가 있으면 읽습니다. 그래서 rwd를 업데이트한 사용자는 따로 설정하지 않아도 다음 `rwd today`부터 Antigravity 대화가 LLM으로 전송됩니다.

이 변경을 사용자가 알 수 있도록, 3단계를 담은 릴리스는 [RELEASES.md](docs/RELEASES.md)의 규칙대로 릴리스 노트 `Highlights`에 아래 세 가지를 적습니다. 이 문서는 설정이나 동작이 바뀌면 `Highlights`에 적으라고 정해 두었습니다. 3단계 PR에는 `enhancement` 라벨을 붙여 `New Features`에도 나오게 합니다.

지금 릴리스 노트는 [generate_release_notes.cjs](../.github/scripts/generate_release_notes.cjs)가 자동으로 만들고, `Highlights`에는 비교 태그, 태그 이름, 설치 방법의 정해진 세 줄만 넣습니다. 손으로 쓴 문장을 넣을 자리가 없습니다. 그래서 3단계에서 이 스크립트를 조금 고칩니다.

- 스크립트는 이미 릴리스에 들어가는 PR마다 `pulls.get`으로 PR 전체를 불러옵니다. 이때 PR 본문에 `## Release Highlights` 제목이 있으면, 그 아래 `- `로 시작하는 줄을 다음 제목이 나올 때까지 모읍니다.
- 모은 줄은 `Highlights`의 정해진 세 줄 앞에 PR 순서대로 넣습니다. `skip-release-notes` 라벨이 붙은 PR의 줄은 넣지 않습니다.
- 이 제목이 없는 PR은 지금과 똑같이 처리합니다.
- 3단계 PR 본문에는 `## Release Highlights` 절을 만들고 아래 예시 세 줄을 넣습니다.

릴리스가 나간 뒤 GitHub에서 릴리스 본문을 직접 고치는 방법도 있지만, 릴리스 때마다 사람이 기억해야 하고 외부 게시물을 손으로 고치는 일이라 고르지 않았습니다.

- rwd가 이제 agy, Antigravity 앱, Antigravity IDE의 대화를 기본으로 읽어 분석에 넣는다는 것
- `agy -p` 같은 비대화형 실행은 기본으로 뺀다는 것
- 끄는 방법: `~/.config/rwd/config.toml`에 `[input.antigravity]`를 만들고 `enabled = false`를 넣는다는 것

릴리스 노트는 지금까지처럼 영어로 씁니다. 3단계 PR 본문에 넣을 예시입니다.

```md
### Highlights
- `rwd today` now reads Antigravity conversations (agy CLI, Antigravity app, Antigravity IDE) by default. Non-interactive `agy -p` runs are skipped.
- To keep Antigravity out of your daily review, add `enabled = false` under `[input.antigravity]` in `~/.config/rwd/config.toml`.
- Run `rwd today --dry-run` to see which sessions will be analyzed without calling the LLM.
```

### 터미널 정보 상자

`commands/display.rs`의 `print_info_box`는 어댑터 목록을 돌며 출처마다 칸 하나를 그립니다. 칸에는 출처 이름, 첫 활동과 마지막 활동 시각, 세션 수를 넣고, `token_usage`가 있으면 토큰 합계도 넣습니다. 로그 폴더를 하나도 못 찾은 출처는 칸을 그리지 않습니다. 폴더는 있지만 그날 세션이 없으면 `No sessions`를 보여 줍니다.

지금은 Claude 칸이 세션이 있을 때만 나오고 Codex 칸은 항상 나옵니다. 위 규칙으로 맞추면 Claude 폴더는 있지만 세션이 없는 날 Claude 칸이 새로 보입니다. 1단계에서 기본 화면이 바뀌는 곳은 이것 하나입니다. 자세히 보기(`-v`) 출력은 출처별로 묶이면서 줄 순서와 문구가 달라질 수 있습니다.

### 확인 옵션 `rwd today --dry-run`

`rwd reset --dry-run`과 같은 이름을 씁니다. `--date`, `--include-automated`, `-v`와 함께 쓸 수 있습니다.

로그 수집까지만 하고, 아래 일은 하지 않습니다.

- LLM 제공자 불러오기, API 키 확인, 요청 한도 조회
- LLM 호출
- 캐시 읽기와 쓰기
- Markdown 파일 쓰기

설정 파일이 없어도 실행됩니다. 이때는 기본 로그 경로만 봅니다.

출력은 정보 상자 다음에 출처별 세션 목록과 뺀 세션 수를 보여 줍니다. 아래 숫자는 모양을 보이려는 예시입니다.

```text
Claude Code   ~/.claude/projects
  c0d5ead8  18:02 ~ 18:41  사용자 12  답변 30  예상 토큰 41,200
  제외: 자동화 세션 2개

Antigravity   ~/.gemini/antigravity/brain 외 2곳
  795fdfbf  18:29 ~ 18:32  사용자 1  답변 10  예상 토큰 9,800
  제외: agy -p 실행 0개

LLM을 부르지 않았습니다. 분석하려면 --dry-run 없이 실행하세요.
```

세션 줄의 사용자, 답변 수는 `SessionTask.messages`에서 역할별로 셉니다. 예상 토큰은 `SessionTask.estimated_tokens`입니다. `-v`를 함께 주면 어댑터의 `verbose_lines`도 출력합니다. 대화 내용은 출력하지 않습니다.

### Antigravity 어댑터

#### 읽는 파일

주 입력은 대화마다 하나씩 있는 `transcript_full.jsonl`입니다. 아래 `brain` 폴더를 순서대로 모으고, 지금처럼 `dedupe_existing_paths`로 중복과 없는 경로를 지웁니다.

| 폴더 | 쓰는 프로그램 |
|---|---|
| 설정의 `input.antigravity.roots` | 사용자가 지정한 `brain` 폴더 |
| `~/.gemini/antigravity-cli/brain` | agy |
| `~/.gemini/antigravity/brain` | Antigravity 앱 |
| `~/.gemini/antigravity-ide/brain` | Antigravity IDE 앱 |
| WSL에서는 `/mnt/c/Users/*/.gemini/` 아래 같은 폴더 | |

각 `brain` 폴더 아래 `<대화 ID>/.system_generated/logs/transcript_full.jsonl`만 읽습니다. 대화 ID는 폴더 이름입니다. 두 앱 폴더 중 어느 쪽을 어느 앱이 쓰는지는 폴더 이름으로 짐작한 것이고, `/Applications`에 `Antigravity.app`과 `Antigravity IDE.app`이 모두 설치돼 있습니다. 어댑터는 두 폴더를 똑같이 읽으므로 이 짐작이 틀려도 동작은 같습니다.

이 파일을 고른 이유는 세 가지입니다.

- 단계 종류가 `USER_INPUT`, `PLANNER_RESPONSE`처럼 이름으로 적혀 있어서, protobuf 필드 번호를 짐작할 필요가 없습니다.
- 한 줄이 JSON 하나라 이미 쓰는 `serde_json`으로 읽을 수 있습니다. 새 크레이트가 필요 없습니다.
- 같은 대화의 SQLite 파일(`conversations/<대화 ID>.db`)과 대조했을 때, 이름과 `step_type` 숫자가 하나씩 짝이 맞았고(14는 `USER_INPUT`, 15는 `PLANNER_RESPONSE`), 답변 100개와 생각 154개가 글자 하나까지 같았습니다.

앞선 초안은 SQLite 파일을 `rusqlite`로 열고 protobuf를 직접 푸는 방식이었습니다. 그 방식은 크레이트 하나와 해석기 하나가 더 필요하고, 필드 뜻을 짐작으로 정해야 해서 버렸습니다.

같은 폴더의 다른 파일은 읽지 않습니다.

- `transcript.jsonl`: 긴 글을 잘라 둔 판입니다. 110줄에 `truncated_fields`가 있었고, 전체판에는 한 줄도 없었습니다.
- `chunks/` 아래 조각 파일: 같은 내용을 나눠 둔 것입니다. 조각이 있는 대화 25개 모두 본파일 줄 수가 조각 줄 수의 합과 같았습니다.
- `conversations/*.db`와 `*.pb`: `*.pb`(2026-05-23에 만든 예전 형식 9개)는 바이트 분포가 무작위 데이터와 같아(엔트로피 8.0) 암호화된 것으로 보입니다.
- `brain/<대화 ID>/` 아래 작업 문서, 그림, `~/.gemini`의 인증 파일(`oauth_creds.json`, `gemini-credentials.json`, `jetski-standalone-oauth-token`)과 설정 파일.

예외는 agy의 `history.jsonl` 하나입니다(아래 `-p` 실행 빼기 절).

알려진 빈틈이 하나 있습니다. SQLite에 있는 단계 940개 중 9개가 `transcript_full.jsonl`에 없었습니다. 7개는 상태 값이 7인 도구 단계(취소나 실패로 보입니다)이고, 1개는 명령 실행, 1개는 `-p` 대화(`0e34cd29`)의 마지막 모델 응답입니다. 그래서 드물게 대화의 마지막 답변이 빠질 수 있습니다. SQLite로 빠진 단계를 채우는 일은 이번 범위에 넣지 않습니다.

파일 수정 시각은 활동 시각으로 쓰지 않고, 대상 날짜의 시작보다 이른 파일을 열지 않는 용도로만 씁니다. 그 뒤로 쓰인 적이 없으면 그날 단계도 있을 수 없기 때문입니다. 이 컴퓨터에서는 8월 23일에 만든 agy 대화 파일 20개의 수정 시각이 2026-10-24 18:15로 바뀌어 있었지만, 안의 단계는 모두 8월 23일 것이었습니다.

#### 줄 구조

`transcript_full.jsonl`의 한 줄은 단계 하나입니다. rwd가 쓰는 필드는 다음 여섯 개입니다.

| 필드 | 내용 |
|---|---|
| `step_index` | 대화 안 단계 번호. 이 순서가 대화 순서입니다 |
| `type` | 단계 종류 |
| `created_at` | 단계 시각. UTC ISO 8601 형식입니다(예: `2026-08-23T04:21:31Z`) |
| `content` | 단계의 글 |
| `thinking` | 모델의 생각 과정. `PLANNER_RESPONSE`에만 있습니다 |
| `tool_calls` | 모델이 부른 도구 목록. 항목마다 `name`과 `args`가 있습니다 |

그 밖의 필드(`source`, `status`, `exit_code` 등)와 모르는 필드는 무시합니다. `status`는 931줄 중 924줄이 `DONE`, 7줄이 `RUNNING`이었습니다. `RUNNING` 단계도 그대로 읽고, 다음 실행 때 지문이 바뀌면 다시 분석됩니다.

rwd가 읽는 단계 종류는 두 가지입니다.

| `type` | 역할 | 읽는 값 | 근거 |
|---|---|---|---|
| `USER_INPUT` | `USER` | `content`에서 `<USER_REQUEST>` 태그 안의 글 | 86개 모두 이 태그가 있었고, 태그 안 글이 SQLite의 사용자 입력과 같았습니다. 태그 밖의 `<ADDITIONAL_METADATA>`, `<USER_SETTINGS_CHANGE>`, `<PLAN>`은 앱이 붙인 정보라 버립니다. 태그가 없으면 `content` 전체를 씁니다 |
| `PLANNER_RESPONSE` | `ASSISTANT`, `TOOL` | `thinking`, `content`, `tool_calls[].name` | 393개. SQLite와 대조해 답변과 생각이 모두 같았습니다 |

나머지 종류(`GENERIC`, `EPHEMERAL_MESSAGE`, `VIEW_FILE`, `RUN_COMMAND`, `CHECKPOINT`, `LIST_DIRECTORY`, `SYSTEM_MESSAGE`, `CONVERSATION_HISTORY`, `CODE_ACTION`, `KNOWLEDGE_ARTIFACTS`, `GREP_SEARCH`, `SEARCH_WEB`, `MCP_TOOL`, `ASK_QUESTION`, `ERROR_MESSAGE` 등)는 도구 실행 결과, 시스템 안내, 대화 요약이라 읽지 않습니다. 도구를 썼다는 사실은 `PLANNER_RESPONSE`의 도구 이름으로 이미 남습니다. 처음 보는 `type`도 읽지 않고 건너뜁니다.

#### 메시지를 프롬프트로 바꾸는 규칙

다른 어댑터와 같은 모양으로 만듭니다.

```text
[Session: 795fdfbf-e0fb-4fc5-85dc-f256a632250c]
[USER] <USER_REQUEST 태그 안의 글>
[ASSISTANT] <thinking>
<content>
[TOOL] run_command
```

- `PLANNER_RESPONSE` 하나에서 생각과 답변은 `ASSISTANT` 한 줄로 합칩니다. Claude 어댑터가 thinking 블록을 답변과 함께 넣는 정책과 같습니다.
- 도구는 이름만 `TOOL` 줄로 남기고 인자(`args`)와 실행 결과는 보내지 않습니다. Codex 어댑터의 정책과 같습니다.
- 작업 폴더를 알려 주는 `META` 줄은 넣지 않습니다. `USER_INPUT` 86개 중 작업 폴더 주소가 들어 있는 것이 없었습니다.
- 모든 글은 `compact_log_like`로 긴 로그를 줄입니다.

#### 날짜와 세션

`created_at`이 대상 날짜의 로컬 0시부터 다음 날 0시 사이인 단계만 씁니다. 남은 단계 중 `USER_INPUT`이나 `PLANNER_RESPONSE`가 하나도 없으면 그 대화는 뺍니다.

세션 ID는 대화 ID(`brain` 아래 폴더 이름)입니다. 같은 ID가 여러 폴더에 있으면 `step_index` 기준으로 합칩니다.

토큰 사용량은 이 파일에 없어서 `token_usage`는 비워 둡니다.

지문의 `session_count`는 남은 대화 수, `entry_count`는 남은 `USER_INPUT`과 `PLANNER_RESPONSE` 단계 수입니다.

#### `-p` 실행 빼기

agy 폴더(`antigravity-cli`)의 대화 중 `~/.gemini/antigravity-cli/history.jsonl`에 대화 ID(`conversationId`)가 없는 대화를 `-p` 실행으로 보고 기본으로 뺍니다. 사용자 입력이 2개 이상인 대화는 이 규칙과 상관없이 대화형으로 봅니다. 두 앱 폴더의 대화에는 이 규칙을 쓰지 않습니다. `include_print_mode = true`면 빼지 않습니다.

이 규칙의 근거는 이 컴퓨터의 agy 대화 25개와, 2026-10-25에 빈 임시 폴더에서 직접 실행한 `agy -p` 한 번입니다.

| 묶음 | 대화 수 | `history.jsonl` | 사용자 입력 |
|---|---|---|---|
| 대화형으로 보이는 대화 | 5 | 모두 있음 | 0개, 1개, 17개, 21개 |
| 2026-08-23에 만든 비대화형 대화 | 20 | 모두 없음 | 모두 1개 |
| 직접 실행한 `agy -p`(`d4a3e4c1`) | 1 | 없음 | 1개 |

직접 실행한 `agy -p`는 새 대화를 만들었지만 `history.jsonl`에는 한 줄도 남기지 않았습니다(실행 전후 모두 50줄). 세 묶음에서 일관되게 갈리는 표시는 `history.jsonl`에 있는지 여부 하나뿐입니다. 단계 순서와 작업 폴더 정보는 `-p` 실행끼리도 달라서 규칙에 쓰지 않습니다. 8월 23일의 20개는 몇 분 간격으로 만들어졌고 같은 시험용 질문이 여러 번 들어 있어서, 다른 프로그램이 agy를 비대화형으로 부른 기록으로 봅니다.

`history.jsonl`이 없거나 읽을 수 없으면 `-p` 실행을 가려낼 수 없습니다. 이때는 모든 대화를 넣고 `-v`에서 알립니다.

`history.jsonl`은 대화 ID만 읽습니다. 그 안의 입력 글(`display`)은 프롬프트에 쓰지 않습니다.

#### 서브에이전트

[공식 문서](https://antigravity.google/docs/subagents?tab=cli)에 따르면 서브에이전트는 부모 에이전트가 `invoke_subagent` 도구로 만들고, 기본 제공 서브에이전트로 research, browser(`/browser` 명령으로만), self가 있습니다. 문서는 서브에이전트 기록이 JSONL 로그로 남는다고 하지만, 부모 대화와 어떻게 이어지는지는 적혀 있지 않습니다. 이 컴퓨터의 대화 40개에는 `invoke_subagent` 호출이 한 번도 없어서 표본이 없습니다.

그래서 3단계의 Antigravity 어댑터는 서브에이전트를 거르지 않고 `brain` 아래 대화 폴더 하나를 세션 하나로 봅니다. 서브에이전트 대화가 따로 폴더를 만든다면, 표본으로 규칙을 정하기 전까지는 별도 세션으로 세질 수 있습니다. 표본을 구하는 방법은 열린 질문 1에 있습니다.

#### 파일 열기

파일은 읽기 전용으로 엽니다. agy나 앱이 쓰고 있어서 마지막 줄이 덜 써졌으면 그 줄만 경고 없이 건너뜁니다. 열리지 않는 파일은 건너뛰고 `-v`에서 파일 이름과 이유를 보여 줍니다. 어떤 경우에도 파일에 쓰지 않습니다.

### 서브에이전트 결과 합치기 (4단계)

지금 rwd는 Claude와 Codex의 서브에이전트 기록을 읽지 않거나 통째로 버립니다. Claude는 프로젝트 폴더 바로 아래의 `*.jsonl`만 읽어서 `<세션 ID>/subagents/` 아래 기록 파일을 아예 열지 않고, 부모 기록에 남은 서브에이전트 결과도 도구 결과로 취급해 버립니다. Codex는 서브에이전트 세션을 빼고, 부모가 받은 결과도 버립니다. 그래서 서브에이전트에게 맡긴 조사나 리뷰에서 나온 결정과 발견이 회고에 남지 않습니다.

4단계부터는 이렇게 바꿉니다.

- 서브에이전트는 지금처럼 세션 목록과 지문에 넣지 않습니다. 같은 작업이 두 번 세지지 않게 하려는 것입니다.
- 부모 세션 프롬프트의 서브에이전트를 부른 자리에 아래 두 줄을 넣습니다.

```text
[SUBAGENT] <서브에이전트 이름이나 종류>: <맡긴 일>
[SUBAGENT_RESULT] <서브에이전트가 돌려준 최종 결과>
```

- 서브에이전트 안의 도구 호출과 중간 대화는 넣지 않습니다. 부모 세션이 실제로 받은 것만 넣는 셈입니다.
- 두 글 모두 `compact_log_like`를 거칩니다.
- 비밀값 가리기는 기존 분석 경로에서 그대로 적용됩니다.

#### 결과 길이 상한

최종 결과는 서브에이전트 하나당 8,000자(`SUBAGENT_RESULT_MAX_CHARS`), 세션 하나에 붙는 서브에이전트 결과 합계는 24,000자(`SUBAGENT_SESSION_MAX_CHARS`)까지 넣습니다. 세션에 서브에이전트가 n개 있고 결과 합계가 24,000자를 넘으면, 하나당 몫을 `min(8,000, 24,000 ÷ n)`자로 줄입니다.

자를 때는 허용 길이의 앞 3분의 2와 끝 3분의 1을 남기고, 가운데에 `[... N자 생략 ...]`을 넣습니다. 보고 끝에 결론을 쓰는 경우가 많다고 봤기 때문인데, 이 비율은 데이터로 확인하지 않은 판단입니다.

이 값은 2026-10-25에 이 컴퓨터의 서브에이전트 최종 결과 72개(Claude 기록 파일 51개, Codex 서브에이전트 세션 21개, 부모 세션 14개)를 재서 정했습니다.

| 하나당 상한 | 잘리는 결과 | 버려지는 글 | 부모 세션 하나에 붙는 최대 글자 수 |
|---|---|---|---|
| 4,000자 | 15개 | 34% | 38,702자 |
| 8,000자 | 6개 | 9% | 47,072자 |
| 10,000자 | 3개 | 4% | 47,072자 |

하나당 4,000자는 결과 다섯 개 중 하나를 자르고 글의 3분의 1을 버려서 너무 짧습니다. 하나당 상한만으로는 프롬프트가 가장 커지는 세션을 막지 못합니다. 그 세션은 결과가 길어서가 아니라 서브에이전트를 26개 불러서 커졌기 때문입니다. 세션 합계 24,000자는 rwd 방식으로 예상 토큰 약 12,000개입니다. 세션이 분당 입력 토큰 한도를 넘으면 조각 요약 경로로 넘어가는데, 지금 그 경로에는 비밀값 가리기가 빠져 있어서 프롬프트가 커지는 것을 더 조심합니다.

#### 출처별로 읽는 곳

| 출처 | 맡긴 일 | 최종 결과 | 부모와 이어지는 방법 | 확인 상태 |
|---|---|---|---|---|
| Claude Code | 서브에이전트 기록 파일의 첫 `user` 항목 글 | 서브에이전트 기록 파일의 마지막 `assistant` 글 | 기록 파일은 `<프로젝트>/<부모 세션 ID>/subagents/agent-<agentId>.jsonl`이고, workflow로 만든 것은 `subagents/workflows/<id>/agent-*.jsonl`입니다. 부모 기록에 같은 `agentId`의 도구 결과가 있으면 그 자리에, 없으면 기록 파일의 첫 시각 자리에 넣습니다 | 부모 기록의 서브에이전트 호출 24개가 모두 같은 세션 폴더에 `agent-<agentId>.jsonl`을 가지고 있었습니다(동기 18개, 비동기 6개). 비동기 6개는 부모 기록에 결과 본문이 없었습니다. 기록 파일 64개 중 40개는 workflow로 만든 것이라 부모 기록에 도구 결과가 없습니다. 기록 파일 첫 줄의 `sessionId`는 부모 세션 ID와 같습니다 |
| Codex | 서브에이전트 세션에서 주입 문맥을 거른 첫 사용자 글. 거르는 규칙은 지금 `extract_codex_input_text`와 같습니다(`# AGENTS.md instructions`, `<environment_context>`, `<external_codex_apps_open_page>`) | 서브에이전트 세션의 마지막 `assistant` 글 | `session_meta`의 `source.subagent.thread_spawn.parent_thread_id`가 부모 세션 ID입니다. 서브에이전트가 다시 부른 서브에이전트는 `parent_thread_id`를 따라 올라가 대화형 부모에 붙입니다. 자리는 서브에이전트 세션의 첫 시각으로 정합니다. 이름은 `agent_nickname`입니다 | `parent_thread_id` 9개가 모두 실제 부모 세션 ID와 같았습니다. 서브에이전트 세션 21개 모두 맨 앞 사용자 메시지가 주입 문맥이었습니다 |
| Antigravity | 표본 없음 | 표본 없음 | 표본 없음 | 표본이 없으면 4단계에서 Antigravity는 빼고 진행합니다(열린 질문 1) |

Claude 기록 파일이 없을 때만 부모 기록의 `toolUseResult.prompt`와 `toolUseResult.content`를 씁니다. 이름은 부모 기록의 `toolUseResult.agentType`을 쓰고, 없으면 workflow 파일은 `workflow`, 그 밖에는 `subagent`로 씁니다.

4단계는 Claude와 Codex의 프롬프트를 일부러 바꾸는 단계입니다. 1단계에서 만든 옮기기 전후 비교 테스트는 서브에이전트가 없는 표본으로 계속 통과해야 합니다.

## 기술 스택

- Rust 2024 Edition, Rust 1.94.0 이상
- 기존 크레이트: `serde`, `serde_json`, `chrono`, `dirs`, `clap`
- 새 크레이트는 추가하지 않습니다. Antigravity의 `transcript_full.jsonl`과 agy의 `history.jsonl`은 모두 `serde_json`으로 읽습니다.

## 명령

모두 `cli/` 폴더에서 실행합니다.

```bash
cargo build
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo run -- doctor
cargo run -- today --dry-run --date 2026-08-23
cargo run -- today --dry-run -v
```

릴리스 노트 스크립트 테스트는 저장소 맨 위 폴더에서 실행합니다.

```bash
node --test .github/scripts/
```

`--dry-run` 없이 `rwd today`를 실행하면 실제 대화가 LLM에 전송되고 비용이 듭니다. 아래 경계를 따릅니다.

## 폴더 구조

```text
cli/src/sources/mod.rs          SourceAdapter, 공통 타입, registered_adapters
cli/src/sources/claude.rs       commands/collection.rs의 Claude 수집 코드와 테스트를 옮긴 어댑터
cli/src/sources/codex.rs        commands/collection.rs의 Codex 수집 코드와 테스트, display.rs의 Codex 활동 시각 계산을 옮긴 어댑터
cli/src/sources/antigravity.rs  Antigravity 어댑터 (폴더 찾기, -p 빼기, 날짜 거르기, SessionTask 만들기)
cli/src/parser/antigravity.rs   Antigravity transcript_full.jsonl과 agy history.jsonl 읽기
cli/src/commands/collection.rs  켜진 어댑터를 차례로 불러 결과를 모으는 함수만 남김
cli/src/commands/today.rs       어댑터 결과로 수집, 지문으로 캐시 비교, --dry-run 분기
cli/src/commands/summary.rs     run_slack의 낡은 캐시 확인을 지문 비교로 바꿈
cli/src/commands/display.rs     SourceCollection으로 출처별 칸 그리기, --dry-run 확인 화면
cli/src/doctor.rs               어댑터 목록으로 로그 폴더 확인
cli/src/cache.rs                지문 표로 바꾼 TodayCache
cli/src/config.rs               InputConfig 옆에 AntigravityInputConfig 추가
cli/src/cli.rs                  today에 --dry-run 추가
cli/src/messages.rs             새 안내 문구
cli/docs/ARCHITECTURE.md        입력 출처 절에 어댑터 구조와 Antigravity 추가
cli/docs/RELEASES.md            PR 본문의 Release Highlights 절 사용법 추가
.github/scripts/generate_release_notes.cjs  PR 본문의 Release Highlights 줄을 Highlights에 넣기
```

의존 방향은 `commands`와 `doctor`에서 `sources`로, `sources`에서 `parser`와 `analyzer`로 향합니다. `commands`와 `doctor`는 `parser::claude`, `parser::codex`를 직접 부르지 않습니다. `analyzer`는 `sources`를 모릅니다. `analyzer::SessionSource` enum은 지우고, 자세히 보기(`-v`) 출력에 쓰던 출처 구분은 `SessionTask`에 출처 id 문자열을 넣어 대신합니다.

## 코드 규칙

[CONVENTIONS.md](docs/CONVENTIONS.md)와 [CLAUDE.md](CLAUDE.md)를 따릅니다. 이번 작업에서는 아래 다섯 가지를 특히 지킵니다.

- 제품 코드에서 `unwrap()`과 `expect()`를 쓰지 않습니다. 어댑터 오류는 `Result`로 돌려주고 `run_today`가 경고로 바꿉니다.
- 로그 구조체의 필드는 `#[serde(default)]`로 받아, 필드가 빠지거나 새로 생겨도 파싱이 멈추지 않게 합니다.
- Antigravity 단계 종류 이름, 태그 이름, 서브에이전트 상한은 이름 있는 상수로 두고, 각 상수 옆 주석에 이 명세의 근거를 가리킵니다. 실제 파일로 확인한 값과 짐작한 값을 주석에서 구분합니다.
- 사용자에게 보이는 문구는 `messages.rs`에 둡니다.
- 경로를 다루는 코드는 Linux, macOS, WSL2에서 동작이 다른지 확인합니다.

```rust
/// User prompt step. Verified: all 86 USER_INPUT lines wrap the typed text
/// in <USER_REQUEST> (SPEC.md, Antigravity line structure).
const STEP_USER_INPUT: &str = "USER_INPUT";
/// Model response step: `content` = answer, `thinking`, `tool_calls[].name`.
/// Verified against SQLite step payloads (100 answers, 154 thoughts).
const STEP_PLANNER_RESPONSE: &str = "PLANNER_RESPONSE";
/// Per-result cap chosen from 72 measured subagent results (SPEC.md, stage 4).
const SUBAGENT_RESULT_MAX_CHARS: usize = 8_000;
```

## 테스트 전략

테스트는 지금처럼 각 모듈 안 `#[cfg(test)]`에 두고, 이름은 `test_<동작>_<조건>_<결과>` 형식을 씁니다. 파일이 필요한 테스트는 임시 폴더에 직접 만든 표본을 씁니다. 사용자의 실제 로그는 저장소에 넣지 않습니다.

1단계 테스트:

- 기존 테스트가 모두 통과합니다. `commands/collection.rs`의 수집 테스트 6개(Claude 1개, Codex 5개)와 `commands/display.rs`의 `test_codex_time_range_ignores_previous_day_session_meta`는 `sources/claude.rs`, `sources/codex.rs`로 옮깁니다. 옮길 때 확인하는 내용은 바꾸지 않고, 활동 시각 테스트는 `CollectedSession.first_activity`와 `last_activity`를 보도록 대상만 바꿉니다.
- 같은 Claude, Codex 표본으로 옮기기 전 수집 함수와 새 어댑터가 만든 `SessionTask`(세션 ID, 프롬프트 글, 토큰 추정치)가 같은지 비교하는 테스트를 둡니다. 옮기기 전 함수를 지우기 전에 이 테스트를 먼저 통과시킵니다.
- 가짜 어댑터 하나가 `Err`를 돌려줘도 다른 어댑터의 세션이 결과에 들어가는지 확인합니다.
- 예전 형식 캐시 JSON을 읽으면 지문이 맞지 않는 것으로 판단하는지, 새 형식은 저장하고 다시 읽어도 같은지 확인합니다.
- 세션이 0개인 출처가 지문 표에 들어가지 않는지 확인합니다.

2단계 테스트:

- `--dry-run`이 LLM 제공자를 불러오지 않는지 확인합니다. 제공자 불러오기를 함수 인자로 받아, 테스트에서는 불리면 실패하는 함수를 넘깁니다.
- `--dry-run`이 캐시 파일과 Markdown 파일을 만들지 않는지 임시 폴더로 확인합니다.
- 설정 파일이 없어도 `--dry-run`이 오류 없이 끝나는지 확인합니다.
- 출력에 대화 글이 들어가지 않는지, 표식 문자열을 넣은 표본으로 확인합니다.

3단계 테스트:

- 테스트용 `transcript_full.jsonl`은 임시 폴더에 `brain/<대화 ID>/.system_generated/logs/` 구조로 직접 만듭니다.
- `USER_INPUT`의 `<USER_REQUEST>` 태그 안 글만 `USER`로 들어가고, `<ADDITIONAL_METADATA>`에 넣은 표식 문자열은 나오지 않습니다. 태그가 없으면 `content` 전체가 들어갑니다.
- `PLANNER_RESPONSE`의 `thinking`과 `content`가 `ASSISTANT`로, `tool_calls[].name`이 `TOOL`로 들어갑니다.
- `tool_calls[].args`와 다른 단계 종류(`RUN_COMMAND` 등)에 넣은 표식 문자열이 프롬프트에 나오지 않습니다. 처음 보는 `type`은 건너뜁니다.
- 같은 폴더의 `transcript.jsonl`과 `chunks/`는 읽지 않습니다.
- 날짜 경계 앞뒤의 단계가 맞게 걸러지고, 수정 시각이 이른 파일은 열지 않습니다. 수정 시각이 늦어도 안의 단계가 다른 날이면 세션이 생기지 않습니다.
- `history.jsonl`에 없는 agy 대화는 빠지고, 있는 대화와 사용자 입력이 2개 이상인 대화는 남습니다. `include_print_mode = true`면 모두 남습니다. `history.jsonl`이 없으면 모두 남습니다.
- 앱 폴더 대화는 `history.jsonl`과 상관없이 남습니다.
- 깨진 줄이나 덜 써진 마지막 줄이 있어도 나머지 줄은 읽힙니다.
- 열 수 없는 파일 하나가 있어도 다른 파일은 읽힙니다.
- `input.antigravity.enabled = false`면 아무것도 수집하지 않습니다. 설정 파일에 `[input.antigravity]`가 없으면 켜진 것으로 봅니다.
- 릴리스 노트 스크립트: 가짜 `github` 객체와 PR 데이터를 넘겨 `node`로 실행합니다. `## Release Highlights` 절의 줄이 `Highlights`에 들어가는지, 절이 없는 PR과 `skip-release-notes` PR은 지금과 같은지 확인합니다. 테스트 파일은 `.github/scripts/generate_release_notes.test.cjs`에 두고 `node --test .github/scripts/`로 실행합니다.

실제 로그 확인: 3단계를 마치면 이 컴퓨터에서 `--dry-run`으로 아래 값이 나오는지 봅니다. LLM을 부르지 않으므로 경계의 먼저 물어볼 일에 들어가지 않습니다. 2026-10-24 18시 40분쯤 분석한 값이라, 그 사이 파일이 지워지거나 오늘 대화가 늘면 달라질 수 있습니다.

| 명령 | 기대하는 Antigravity 결과 |
|---|---|
| `rwd today --dry-run --date 2026-08-23` | 세션 7개(앱 2개, IDE 5개), `-p` 실행으로 뺀 대화 20개 |
| `rwd today --dry-run --date 2026-10-24` | 세션 1개(`795fdfbf`). 파일 수정 시각만 10월 24일로 바뀐 8월 agy 대화 20개는 나오지 않습니다 |
| `rwd today --dry-run --date 2026-10-25` | `-p` 실행으로 뺀 대화 1개(`d4a3e4c1`). 그날 다른 Antigravity 대화가 없다면 세션은 0개입니다 |

4단계 테스트:

- Claude: `<부모 세션 ID>/subagents/agent-<agentId>.jsonl`의 첫 `user` 글과 마지막 `assistant` 글이 부모 기록의 같은 `agentId` 도구 결과 자리에 `SUBAGENT`, `SUBAGENT_RESULT` 줄로 들어갑니다. 기록 파일 안의 도구 호출에 넣은 표식 문자열은 나오지 않습니다.
- Claude: 부모 기록에 결과 본문이 없는 비동기 실행도 기록 파일로 결과가 들어갑니다. 기록 파일이 없으면 부모 기록의 `toolUseResult.prompt`와 `content`를 씁니다.
- Claude: `subagents/workflows/<id>/` 아래 기록 파일은 부모 기록에 도구 결과가 없어도 첫 시각 자리에 들어갑니다.
- Codex: 서브에이전트 세션의 주입 문맥을 거른 첫 사용자 글과 마지막 답변이 `parent_thread_id`의 부모 세션에 들어가고, 서브에이전트는 세션 수에 들어가지 않습니다. 서브에이전트가 다시 부른 서브에이전트는 대화형 부모까지 올라가 붙습니다.
- 결과가 8,000자를 넘으면 앞 3분의 2와 끝 3분의 1만 남고 가운데에 생략 표시가 들어갑니다.
- 결과 합계가 24,000자를 넘는 세션에서는 하나당 몫이 `min(8,000, 24,000 ÷ 서브에이전트 수)`로 줄고, 합계가 24,000자를 넘지 않습니다.
- 서브에이전트가 없는 세션의 프롬프트는 3단계와 같습니다.

## 경계

항상 할 일:

- 커밋 전에 `cargo build`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked`를 통과시킵니다.
- 1단계에서는 Claude Code와 Codex의 수집 결과와 프롬프트를 바꾸지 않습니다. 바뀌는 화면은 정보 상자 규칙 하나뿐입니다.
- 에이전트 로그 파일은 읽기 전용으로만 엽니다.
- 실제 로그로 확인할 때는 `--dry-run`을 씁니다.

먼저 물어볼 일:

- 크레이트 추가
- 형식 확인을 위해 `agy -p`나 Antigravity를 다시 실행하는 일. 사용자 계정으로 요청이 나가고 사용량이 줄어듭니다. 2026-10-25의 한 번은 승인받아 실행했습니다.
- 사용자의 실제 로그로 `--dry-run` 없이 `rwd today`를 실행하는 일. 대화가 LLM으로 전송되고 비용이 듭니다.
- 캐시 파일 이름이나 위치 변경
- 기존 출처의 Markdown 제목(`Claude Code`, `Codex`) 변경
- `rwd config` 명령에 새 키 추가

하지 않을 일:

- `~/.gemini`의 인증 파일과 설정 파일을 읽는 일
- `brain` 폴더에서 `transcript_full.jsonl` 말고 다른 파일을 읽는 일
- 예전 `.pb` 대화 파일을 풀려고 시도하는 일
- 에이전트 로그 파일에 쓰는 일
- 도구 호출 인자와 실행 결과를 LLM에 보내는 일
- 사용자의 실제 로그를 테스트 표본으로 커밋하는 일
- `unsafe` 사용
- 실패하는 기존 테스트를 지우거나 건너뛰는 일

## 이번 범위가 아닌 것

- Gemini CLI 어댑터. 필요해지면 이 구조 위에 따로 명세를 씁니다.
- Cursor 등 다른 에이전트 어댑터
- 예전 `.pb` 형식 Antigravity 대화
- `transcript_full.jsonl`에 빠진 단계를 SQLite 파일(`conversations/*.db`)로 채우는 일
- Antigravity 토큰 사용량 표시
- 실행 중에 외부 플러그인을 불러오는 방식
- 분석 단계(프롬프트, LLM 호출, 비밀값 가리기)와 분석 실패 처리 변경
- 클라우드에서만 도는 에이전트 작업(ChatGPT 앱의 Codex 클라우드 작업 등)

## 완료 조건

1단계:

1. `cargo test --locked`가 통과하고, 옮기기 전후 `SessionTask` 비교 테스트가 통과합니다.
2. `commands/`와 `doctor.rs`의 테스트가 아닌 코드에 `parser::claude`, `parser::codex` 같은 출처별 파서 참조가 남아 있지 않습니다. 지금은 41줄입니다(현재 구조 절의 표 아래).
3. 어댑터 하나를 추가할 때 고치는 곳이 `sources/` 아래 새 파일, `registered_adapters`의 한 줄, `config.rs`의 설정 구조체 하나뿐입니다. `commands/`와 `doctor.rs`는 고치지 않습니다.
4. 수집 중 어댑터 하나가 실패해도 나머지 출처는 분석되고, 실패한 출처 이름이 경고로 나옵니다.
5. 예전 캐시가 있는 날짜는 한 번 다시 분석되고, 그다음 실행부터는 캐시를 씁니다.

2단계:

1. `rwd today --dry-run`이 출처별 세션 목록과 뺀 세션 수를 보여 주고 끝납니다.
2. `--dry-run`은 LLM 제공자를 불러오지 않고, 캐시와 Markdown 파일을 쓰지 않으며, 설정 파일이 없어도 동작합니다.
3. 출력에 대화 글이 나오지 않습니다.

3단계:

1. 대상 날짜에 Antigravity 대화가 있으면 터미널 결과와 Obsidian 파일에 `Antigravity` 절이 생깁니다.
2. `rwd today -v`, `rwd today --dry-run`, `rwd doctor`가 찾은 Antigravity 폴더를 보여 줍니다.
3. `-p` 실행으로 보이는 대화, 도구 인자와 실행 결과, `USER_INPUT`과 `PLANNER_RESPONSE`가 아닌 단계의 글, `<USER_REQUEST>` 밖의 메타데이터가 프롬프트에 들어가지 않습니다.
4. 위 3단계 테스트가 모두 통과하고, 실제 로그 확인 표의 값이 나옵니다.
5. Antigravity 기록이 없는 컴퓨터에서는 화면과 결과가 2단계와 같습니다.
6. 설정 파일에 `[input.antigravity]`가 없어도 Antigravity 대화를 읽고, `enabled = false`면 읽지 않습니다.
7. 릴리스 노트 스크립트가 PR 본문의 `## Release Highlights` 줄을 `Highlights`에 넣고, 3단계 PR 본문에 그 절이 있습니다. 그래서 3단계를 담은 릴리스의 릴리스 노트에 기본으로 읽는다는 것, `-p` 실행을 뺀다는 것, 끄는 방법이 나옵니다.

4단계:

1. Claude와 Codex 세션에서 서브에이전트에게 맡긴 일과 최종 결과가 부모 세션 프롬프트에 들어갑니다.
2. 서브에이전트는 세션 수와 지문에 들어가지 않습니다.
3. 서브에이전트 안의 도구 호출과 중간 대화는 프롬프트에 들어가지 않습니다.
4. 서브에이전트 결과는 하나당 8,000자, 세션 합계 24,000자를 넘지 않습니다.
5. Claude는 `subagents/` 아래 기록 파일을 먼저 읽고, Codex는 `parent_thread_id`로 부모를 찾습니다.
6. 위 4단계 테스트가 모두 통과합니다.

## 열린 질문

남은 열린 질문은 없습니다. 아래 1번은 2026-10-25에 표본 없이 진행하기로 정했고, 나중에 Antigravity를 4단계에 넣을 때 참고하도록 남겨 둡니다.

1. Antigravity 서브에이전트 표본을 구하는 방법은 세 가지입니다. 첫 번째를 먼저 하고, 멈추면 두 번째로 가는 순서를 권합니다.
   - 빈 임시 폴더에서 `agy -p`로 기본 제공 research 서브에이전트를 불러 달라고 요청합니다. 제한 시간(`--print-timeout`)을 걸어 실행합니다. 사용자 계정으로 요청이 나가므로 승인이 필요하고, 서브에이전트가 승인을 기다리면 멈출 수 있습니다. 이렇게 생긴 대화는 `-p` 규칙으로 rwd 분석에서 빠지지만, 하위 대화가 어떤 폴더와 파일로 생기는지 확인하기에는 충분합니다.
   - Antigravity 앱에서 사용자가 직접 research 서브에이전트를 쓰는 작업을 한 번 합니다. rwd가 실제로 읽을 대화형 기록과 같은 모양입니다.
   - 임시 폴더의 `.agents/agents/<이름>.md`에 서브에이전트 정의를 만들고 `agy -p`로 부릅니다. 이름이 정해져 있어서 기록에서 찾기 쉽습니다.

   표본이 없으면 3단계는 지금 규칙으로 진행하고, 4단계에서 Antigravity는 뺍니다.
