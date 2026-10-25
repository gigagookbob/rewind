# 작업 목록: 여러 AI 에이전트 로그를 읽는 어댑터 구조

기준 명세: [SPEC.md](../SPEC.md), 계획: [plan.md](plan.md)

모든 명령은 `cli/` 폴더에서 실행합니다. 작업마다 공통으로 아래 세 명령을 통과해야 끝난 것으로 봅니다.

```bash
cargo build
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

작업이 끝나면 그 작업의 파일만 골라 로컬 커밋합니다.

## 1단계: 구조 정리

### 작업 1: 어댑터 계약과 Claude 어댑터 만들기

**설명:** 새 `sources` 모듈에 명세의 어댑터 계약(`SourceAdapter`, `CollectContext`, `SourceCollection`, `CollectedSession`, `SourceFingerprint`, `registered_adapters`)을 만들고, Claude 어댑터를 넣습니다. Claude 어댑터는 `commands/collection.rs`의 `collect_claude_entries_with_stats` 흐름(폴더 찾기, 날짜 거르기, 중복 지우기, 자동화 세션 빼기, 정렬)을 그대로 옮기고, `analyzer::build_claude_session_tasks`로 `SessionTask`를 만듭니다. 활동 시각과 토큰 합계도 채웁니다. 이 작업에서는 `commands/`를 바꾸지 않습니다. `sources` 모듈에는 작업 4까지만 `#[allow(dead_code)]`를 둡니다.

**수용 기준:**
- [ ] 같은 임시 폴더 표본에서 `ClaudeAdapter::with_roots(...).collect(...)`가 만든 `SessionTask`의 세션 ID, `prompt_text`, `estimated_tokens`가 옛 함수(`collect_claude_entries` 다음 `build_claude_session_tasks`) 결과와 같습니다. 이 비교 테스트는 옛 함수가 있는 `commands/collection.rs`의 테스트 모듈에 둡니다.
- [ ] `test_collect_claude_entries_dedupes_across_roots`와 같은 내용의 테스트가 `sources/claude.rs`에 있고 통과합니다. 표본의 `prompt_text`를 그대로 적은 고정 테스트도 하나 둡니다.
- [ ] 자동화 세션은 `excluded`에 `automated`로 세어지고, `fingerprint.entry_count`는 지금의 `claude_entry_count`와 같은 값입니다.

**확인:**
- [ ] `cargo test --locked sources::claude`
- [ ] `cargo test --locked commands::collection`
- [ ] 공통 세 명령

**의존:** 없음

**고칠 파일:**
- `src/sources/mod.rs` (새 파일)
- `src/sources/claude.rs` (새 파일)
- `src/main.rs` (`mod sources;` 한 줄)
- `src/commands/collection.rs` (비교 테스트만)

**크기:** M

### 작업 2: Codex 어댑터 만들기

**설명:** `commands/collection.rs`의 `collect_codex_sessions` 흐름(UTC 날짜 폴더 찾기, 세션 ID로 합치기, 서브에이전트 세션 빼기, 정렬)과 `commands/display.rs`의 Codex 활동 시각 계산을 `sources/codex.rs`로 옮기고, `analyzer::build_codex_session_task`로 `SessionTask`를 만듭니다. `registered_adapters`에 등록합니다.

**수용 기준:**
- [ ] 같은 표본에서 Codex 어댑터 결과(세션 ID, `prompt_text`, `estimated_tokens`)가 옛 함수 결과와 같다는 비교 테스트가 `commands/collection.rs` 테스트 모듈에서 통과합니다.
- [ ] `commands/collection.rs`의 Codex 수집 테스트 5개와 `commands/display.rs`의 `test_codex_time_range_ignores_previous_day_session_meta`와 같은 내용의 테스트가 `sources/codex.rs`에 있고 통과합니다. 활동 시각 테스트는 `CollectedSession.first_activity`와 `last_activity`를 봅니다.
- [ ] 서브에이전트 세션은 `excluded`에 `subagent`로 세어지고, 지문은 지금의 `codex_session_count`, `codex_entry_count`와 같은 값입니다.

**확인:**
- [ ] `cargo test --locked sources::codex`
- [ ] `cargo test --locked commands::collection`
- [ ] 공통 세 명령

**의존:** 작업 1

**고칠 파일:**
- `src/sources/codex.rs` (새 파일)
- `src/sources/mod.rs`
- `src/commands/collection.rs` (비교 테스트만)

**크기:** S

### 작업 3: `run_today`, `run_slack`, 캐시, 정보 상자를 어댑터로 바꾸기

**설명:** `commands/`가 어댑터 목록만 쓰도록 바꿉니다. `commands/collection.rs`에는 어댑터 목록을 인자로 받아 켜진 어댑터를 차례로 부르고, 실패한 어댑터는 이름과 함께 경고로 바꾸는 함수 하나만 남깁니다. 옛 Claude, Codex 수집 함수와 작업 1, 2의 비교 테스트는 지웁니다. `run_today`는 출처별 결과로 정보 상자, 캐시 비교, 분석, 캐시 저장을 합니다. `TodayCache`는 `source_fingerprints` 표로 바꾸고, 표가 없는 예전 캐시는 맞지 않는 것으로 봅니다. `run_slack`의 낡은 캐시 확인도 지문 표를 씁니다. 정보 상자는 출처마다 칸 하나를 그리고, 출처별 활동 시각 함수는 지웁니다.

**수용 기준:**
- [ ] `cache.rs` 테스트: 예전 형식 캐시 JSON은 지문이 맞지 않는 것으로 판단하고, 새 형식은 저장하고 다시 읽어도 같으며, 세션이 0개인 출처는 지문 표에 들어가지 않습니다.
- [ ] 가짜 어댑터 하나가 `Err`를 돌려줘도 다른 가짜 어댑터의 세션이 결과에 들어가고, 실패한 출처 이름이 경고 목록에 남는 테스트가 통과합니다.
- [ ] `src/commands/`의 테스트가 아닌 코드에 `parser::claude`, `parser::codex` 참조가 없습니다. 정보 상자 칸을 만드는 함수 테스트에서, 폴더는 있지만 세션이 없는 출처는 `No sessions` 칸이 되고 폴더가 없는 출처는 칸이 없습니다.

**확인:**
- [ ] `cargo test --locked cache`, `cargo test --locked commands`
- [ ] 작업 1, 2의 고정 테스트(`sources::claude`, `sources::codex`)가 그대로 통과
- [ ] `grep -n "parser::claude\|parser::codex" src/commands/*.rs`에 테스트 모듈 밖의 결과가 없음
- [ ] 공통 세 명령

**의존:** 작업 1, 2

**고칠 파일:**
- `src/commands/today.rs`
- `src/commands/summary.rs`
- `src/commands/collection.rs`
- `src/commands/display.rs`
- `src/cache.rs`

**크기:** M (다섯 파일. 이 작업에서는 출처별 흐름을 바꾸는 일 말고 다른 정리는 하지 않습니다)

### 작업 4: `rwd doctor`, `SessionSource` 정리와 임시 허용 지우기

**설명:** `doctor.rs`의 로그 폴더 확인을 어댑터 목록으로 바꿉니다. `analyzer::SessionSource` enum을 지우고 `SessionTask`에 출처 id 문자열을 넣습니다. `-v`에서 쓰던 출처별 작업 수 표시는 이 id로 셉니다. `sources` 모듈의 `#[allow(dead_code)]`를 지웁니다.

**수용 기준:**
- [ ] `doctor.rs`와 `src/commands/`의 테스트가 아닌 코드에 `parser::claude`, `parser::codex` 참조가 0줄입니다(작업 전 41줄).
- [ ] `src/sources/` 아래에 `allow(dead_code)`가 없고 Clippy 경고가 없습니다.
- [ ] `rwd doctor`의 로그 폴더 줄을 만드는 함수가 어댑터마다 이름과 찾은 폴더 수를 한 줄씩 만든다는 테스트가 통과합니다.

**확인:**
- [ ] `cargo test --locked doctor`, `cargo test --locked analyzer`
- [ ] `grep -rn "allow(dead_code)" src/sources/` 결과 없음
- [ ] `cargo run -- doctor`로 Claude Code, Codex 폴더 줄이 나오는지 직접 확인(LLM을 부르지 않음)
- [ ] 공통 세 명령과 `cargo fmt --check`

**의존:** 작업 3

**고칠 파일:**
- `src/doctor.rs`
- `src/analyzer/mod.rs`
- `src/sources/mod.rs`
- `src/sources/claude.rs`
- `src/sources/codex.rs`

**크기:** M

### 확인 지점 1: 1단계 끝

- [ ] 공통 세 명령과 `cargo fmt --check` 통과
- [ ] 명세 1단계 완료 조건 1~5를 하나씩 확인
- [ ] 사용자와 함께 결과를 보고 1단계 PR 여부 결정

## 2단계: 확인 옵션

### 작업 5: `rwd today --dry-run` 넣기

**설명:** `today`에 `--dry-run`을 추가합니다. `-b`, `--worker`와 함께 쓰면 clap이 오류를 냅니다. `main.rs`는 `--dry-run`이면 설정 확인, 언어 선택, LLM 제공자 불러오기 전에 별도 함수로 갑니다. 이 함수는 어댑터 목록과 수집 조건만 받아 수집하고, 정보 상자 다음에 출처별 세션 목록(세션 ID 앞 8자리, 시각 범위, 사용자와 답변 수, 예상 토큰)과 뺀 세션 수를 출력하고 끝납니다. 출력 글은 순수 함수가 만들고 화면 출력만 따로 합니다. 설정 파일이 없으면 기본 경로만 봅니다.

**수용 기준:**
- [ ] clap 파싱 테스트: `rwd today --dry-run`은 통과하고, `rwd today --dry-run -b`는 오류입니다.
- [ ] 출력 함수 테스트: 세션 글에 넣은 표식 문자열이 출력에 없고, 세션 수, 뺀 세션 수, `LLM을 부르지 않았다`는 안내가 있습니다.
- [ ] 설정 없이(`config: None`) 임시 폴더 어댑터로 확인 함수를 부르면 오류 없이 끝납니다. 확인 함수는 LLM 제공자, 캐시, Markdown 저장 함수를 인자로 받지 않고 부르지도 않습니다(코드 검토로 확인).

**확인:**
- [ ] `cargo test --locked cli`, `cargo test --locked commands`
- [ ] `cargo run -- today --dry-run`과 `cargo run -- today --dry-run -v`를 이 컴퓨터에서 직접 실행해, Claude Code와 Codex 세션 목록이 나오고 분석 안내 문구(`Analyzing...`)가 나오지 않는지 봅니다
- [ ] 공통 세 명령

**의존:** 작업 4

**고칠 파일:**
- `src/cli.rs`
- `src/main.rs`
- `src/commands/today.rs`
- `src/commands/display.rs`
- `src/messages.rs`

**크기:** M

### 확인 지점 2: 2단계 끝

- [ ] 명세 2단계 완료 조건 1~3을 하나씩 확인
- [ ] 사용자와 함께 결과를 보고 2단계 PR 여부 결정

## 3단계: Antigravity 어댑터

### 작업 6: Antigravity 기록 읽기

**설명:** `parser/antigravity.rs`에 `transcript_full.jsonl` 한 줄을 받는 구조체(`step_index`, `type`, `created_at`, `content`, `thinking`, `tool_calls`)와 파일 읽기 함수를 만듭니다. 모든 필드는 `#[serde(default)]`로 받고, 깨진 줄은 경고 없이 건너뜁니다. `content`에서 `<USER_REQUEST>` 태그 안 글을 꺼내는 함수와, agy `history.jsonl`에서 `conversationId` 목록을 읽는 함수도 만듭니다.

**수용 기준:**
- [ ] `<USER_REQUEST>` 안 글만 꺼내고, 태그가 없으면 `content` 전체를 돌려줍니다. `<ADDITIONAL_METADATA>` 안 표식 문자열은 결과에 없습니다.
- [ ] 깨진 줄과 덜 써진 마지막 줄이 있어도 나머지 줄은 읽히고, 모르는 필드와 처음 보는 `type`이 있어도 읽기가 멈추지 않습니다.
- [ ] `history.jsonl`이 있으면 대화 ID 목록을, 없거나 읽을 수 없으면 `None`을 돌려줍니다. `display` 글은 돌려주지 않습니다.

**확인:**
- [ ] `cargo test --locked parser::antigravity`
- [ ] 공통 세 명령

**의존:** 없음(1단계와 함께 진행할 수 있습니다)

**고칠 파일:**
- `src/parser/antigravity.rs` (새 파일)
- `src/parser/mod.rs`

**크기:** S

### 작업 7: Antigravity 어댑터와 설정

**설명:** `sources/antigravity.rs`에 명세의 Antigravity 어댑터를 만듭니다. 설정의 `roots`, `~/.gemini/antigravity-cli/brain`, `~/.gemini/antigravity/brain`, `~/.gemini/antigravity-ide/brain`, WSL의 Windows 홈 아래 같은 폴더를 찾고, 각 폴더의 `<대화 ID>/.system_generated/logs/transcript_full.jsonl`만 읽습니다. 수정 시각이 대상 날짜 시작보다 이른 파일은 열지 않고, `created_at`으로 날짜를 거르고, agy 폴더의 대화 중 `history.jsonl`에 없고 사용자 입력이 1개 이하인 대화를 `print-mode`로 뺍니다. 프롬프트는 `USER`, `ASSISTANT`(생각과 답변), `TOOL`(도구 이름) 줄로 만들고 `compact_log_like`를 거칩니다. `config.rs`에 `AntigravityInputConfig`(`enabled` 기본 `true`, `roots`, `include_print_mode` 기본 `false`)를 넣고 어댑터를 등록합니다.

**수용 기준:**
- [ ] 명세의 3단계 테스트 목록(태그 처리, 도구 인자와 다른 단계 종류 제외, `transcript.jsonl`과 `chunks/` 무시, 날짜 경계, 수정 시각, `-p` 규칙 네 가지 경우, 앱 폴더 예외, 깨진 줄, 열 수 없는 파일)이 모두 통과합니다.
- [ ] 같은 대화 ID가 두 폴더에 있으면 `step_index` 기준으로 한 세션이 됩니다.
- [ ] `[input.antigravity]`가 없으면 켜진 것으로, `enabled = false`면 아무것도 수집하지 않는 것으로 동작합니다.

**확인:**
- [ ] `cargo test --locked sources::antigravity`, `cargo test --locked config`
- [ ] 공통 세 명령

**의존:** 작업 4, 작업 6

**고칠 파일:**
- `src/sources/antigravity.rs` (새 파일)
- `src/sources/mod.rs`
- `src/config.rs`
- `src/messages.rs`

**크기:** M

### 작업 8: 문서와 실제 로그 확인

**설명:** `docs/ARCHITECTURE.md`의 입력 출처와 폴더 구조에 어댑터 구조와 Antigravity를 넣고, `README.md`의 입력 설정 절에 Antigravity 지원, `[input.antigravity]` 설정, `--dry-run` 사용법을 넣습니다. 그다음 이 컴퓨터의 실제 로그로 명세의 확인 표를 돌려 봅니다.

**수용 기준:**
- [ ] `ARCHITECTURE.md`와 `README.md`가 지금 코드 구조와 설정을 설명합니다.
- [ ] `cargo run -- today --dry-run --date 2026-08-23`에서 Antigravity 세션 7개(앱 2개, IDE 5개)와 `-p` 실행으로 뺀 대화 20개가 나옵니다.
- [ ] `--date 2026-10-24`에서 세션 1개(`795fdfbf`), `--date 2026-10-25`에서 `-p` 실행으로 뺀 대화 1개(`d4a3e4c1`)가 나옵니다. 그 사이 파일이 지워졌거나 대화가 늘었으면 차이를 기록합니다.

**확인:**
- [ ] 위 세 명령을 직접 실행(LLM을 부르지 않음)
- [ ] `cargo run -- doctor`에 Antigravity 폴더 줄이 나옴

**의존:** 작업 5, 작업 7

**고칠 파일:**
- `docs/ARCHITECTURE.md`
- `README.md`

**크기:** S

### 작업 9: 릴리스 노트에 `Release Highlights` 넣기

**설명:** `.github/scripts/generate_release_notes.cjs`가 릴리스에 들어가는 PR 본문의 `## Release Highlights` 절에서 `- `로 시작하는 줄을 다음 제목까지 모아, `Highlights`의 정해진 세 줄 앞에 PR 순서대로 넣게 합니다. `skip-release-notes` PR의 줄은 넣지 않습니다. `docs/RELEASES.md`에 이 절을 쓰는 방법을 적습니다.

**수용 기준:**
- [ ] `node --test`로 도는 테스트에서, 절이 있는 PR의 줄이 `Highlights`에 들어가고, 절이 없는 PR만 있으면 출력이 지금과 같고, `skip-release-notes` PR의 줄은 들어가지 않습니다.
- [ ] 스크립트 테스트는 가짜 `github` 객체와 PR 데이터만 쓰고 네트워크를 쓰지 않습니다.
- [ ] `RELEASES.md`에 절 이름, 줄 형식, 예시가 있습니다.

**확인:**
- [ ] 저장소 맨 위 폴더에서 `node --test .github/scripts/`

**의존:** 없음(3단계 PR보다 먼저 `main`에 들어가야 합니다)

**고칠 파일:**
- `../.github/scripts/generate_release_notes.cjs`
- `../.github/scripts/generate_release_notes.test.cjs` (새 파일)
- `docs/RELEASES.md`

**크기:** S

### 확인 지점 3: 3단계 끝

- [ ] 명세 3단계 완료 조건 1~7을 하나씩 확인
- [ ] 3단계 PR 본문에 `## Release Highlights` 절(명세의 예시 세 줄)과 `enhancement` 라벨
- [ ] 사용자와 함께 결과를 보고 3단계 PR 여부 결정

## 4단계: 서브에이전트 결과 합치기 (Claude, Codex)

### 작업 10: 서브에이전트 줄과 길이 상한

**설명:** `sources/subagent.rs`에 서브에이전트 한 건(이름, 맡긴 일, 최종 결과, 시각)을 담는 타입과 길이 상한 함수를 만듭니다. 상한은 하나당 8,000자, 세션 합계 24,000자이고, 합계를 넘으면 하나당 몫을 `min(8,000, 24,000 ÷ 서브에이전트 수)`로 줄입니다. 자를 때는 허용 길이의 앞 3분의 2와 끝 3분의 1을 남기고 가운데에 `[... N자 생략 ...]`을 넣습니다. `analyzer/prompt.rs`의 Claude, Codex 프롬프트 조립 함수가 서브에이전트 목록을 함께 받아, 각 줄의 시각보다 늦은 첫 메시지 앞에 `[SUBAGENT]`, `[SUBAGENT_RESULT]` 줄을 넣게 합니다.

**수용 기준:**
- [ ] 8,000자를 넘는 결과가 앞 3분의 2와 끝 3분의 1, 생략 표시로 잘리고, 남은 글자 수가 허용 길이를 넘지 않습니다.
- [ ] 결과 합계가 24,000자를 넘는 세션에서 하나당 몫이 줄고, 남은 결과 글자 수의 합이 24,000자를 넘지 않습니다.
- [ ] 서브에이전트 목록이 비어 있으면 프롬프트가 작업 1, 2의 고정 테스트와 똑같습니다.

**확인:**
- [ ] `cargo test --locked sources::subagent`, `cargo test --locked analyzer::prompt`
- [ ] `cargo test --locked sources` (고정 테스트 포함)
- [ ] 공통 세 명령

**의존:** 작업 4

**고칠 파일:**
- `src/sources/subagent.rs` (새 파일)
- `src/sources/mod.rs`
- `src/analyzer/prompt.rs`
- `src/analyzer/mod.rs`

**크기:** M

### 작업 11: Claude 서브에이전트 붙이기

**설명:** `parser/claude.rs`의 사용자 항목에 `toolUseResult`를 선택 필드로 받습니다. Claude 어댑터는 그날 세션마다 `<프로젝트>/<세션 ID>/subagents/` 아래(하위 폴더 포함)의 `agent-*.jsonl`을 읽어, 첫 `user` 글을 맡긴 일로, 마지막 `assistant` 글을 최종 결과로 씁니다. 부모 기록에 같은 `agentId`의 도구 결과가 있으면 그 시각에, 없으면 기록 파일의 첫 시각에 붙입니다. 이름은 부모 기록의 `agentType`, 없으면 workflow 파일은 `workflow`, 그 밖은 `subagent`입니다. 기록 파일이 없을 때만 부모 기록의 `toolUseResult.prompt`와 `content`를 씁니다. `-v`에 붙인 서브에이전트 수를 보여 줍니다.

**수용 기준:**
- [ ] 명세 4단계의 Claude 테스트 세 가지(기록 파일 우선과 자리, 비동기와 기록 파일이 없을 때의 대체, workflow 파일)가 통과합니다.
- [ ] 기록 파일 안의 도구 호출에 넣은 표식 문자열은 프롬프트에 없고, 서브에이전트는 세션 수와 지문에 들어가지 않습니다.
- [ ] 서브에이전트가 없는 세션의 프롬프트는 작업 1의 고정 테스트와 같습니다.

**확인:**
- [ ] `cargo test --locked sources::claude`, `cargo test --locked parser::claude`
- [ ] `cargo run -- today --dry-run -v`에서 Claude의 붙인 서브에이전트 수가 나옴(LLM을 부르지 않음)
- [ ] 공통 세 명령

**의존:** 작업 10

**고칠 파일:**
- `src/parser/claude.rs`
- `src/sources/claude.rs`

**크기:** S

### 작업 12: Codex 서브에이전트 붙이기

**설명:** `parser/codex.rs`의 `SessionMeta`에서 `source.subagent.thread_spawn.parent_thread_id`를 꺼내 둡니다. Codex 어댑터는 서브에이전트 세션을 세션 목록과 지문에서 계속 빼되 따로 모아, `parent_thread_id`를 따라 대화형 부모까지 올라가 붙입니다. 맡긴 일은 주입 문맥을 거른 첫 사용자 글, 최종 결과는 마지막 답변, 시각은 서브에이전트 세션의 첫 시각, 이름은 `agent_nickname`입니다. `-v`에 붙인 서브에이전트 수를 보여 줍니다.

**수용 기준:**
- [ ] 서브에이전트 세션의 첫 사용자 글과 마지막 답변이 부모 세션 프롬프트에 들어가고, 서브에이전트는 세션 수와 지문에 들어가지 않습니다.
- [ ] 서브에이전트가 부른 서브에이전트(깊이 2)는 대화형 부모에 붙습니다. 부모를 찾지 못한 서브에이전트는 붙이지 않고 `-v`에 수만 보여 줍니다.
- [ ] 서브에이전트가 없는 세션의 프롬프트는 작업 2의 고정 테스트와 같습니다.

**확인:**
- [ ] `cargo test --locked sources::codex`, `cargo test --locked parser::codex`
- [ ] `cargo run -- today --dry-run -v`에서 Codex의 붙인 서브에이전트 수가 나옴(LLM을 부르지 않음)
- [ ] 공통 세 명령

**의존:** 작업 10

**고칠 파일:**
- `src/parser/codex.rs`
- `src/sources/codex.rs`

**크기:** S

### 확인 지점 4: 4단계 끝

- [ ] 명세 4단계 완료 조건 1~6을 하나씩 확인
- [ ] `--dry-run -v`의 예상 토큰으로, 서브에이전트를 많이 쓴 세션이 서브에이전트 글 때문에 약 12,000토큰 넘게 늘지 않는지 확인
- [ ] 사용자와 함께 결과를 보고 4단계 PR 여부 결정
