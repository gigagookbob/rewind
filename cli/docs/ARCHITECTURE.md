# Architecture — rwd

## Core Flow

```
rwd today:       CLI entry → source adapters (log discovery, JSONL parsing, session tasks) → cache check → LLM provider call (API or Codex CLI) → Markdown generation → Obsidian Vault save
rwd today --dry-run: CLI entry → source adapters → per-source session list → exit (no LLM provider, cache, or Markdown)
rwd today -b:    CLI entry → spawn background worker → exit immediately → OS notification on completion
rwd summary:     CLI entry → cache load → collect work_summaries → LLM API (SUMMARY_PROMPT) → Obsidian save + clipboard copy
rwd slack:       CLI entry → cache load → collect work_summaries → LLM API (SLACK_PROMPT) → clipboard copy
```

## Extracted Insights

- User's decision branches (why A over B)
- Things the user was curious or confused about
- Model errors corrected by the user
- Context switches between sessions (which project, which task)

### 회고 근거의 연결

파서는 사용자 발언과 AI 응답에 `SourceLocation { path, line }`을 붙입니다. 위치는 로그 페이로드에서 받지 않고 파일을 읽을 때 정합니다. 날짜 필터, 중복 제거와 시간순 정렬 뒤에도 선택된 원본 위치를 보존합니다. Codex의 중복 사용자 기록과 여러 루트에서 발견한 같은 기록은 기존 중복 제거 규칙으로 선택한 파일과 줄을 가리킵니다.

`SessionTask.evidence_sources`는 분석 입력의 발언 순번, 원래 대화 텍스트, 압축된 입력 텍스트와 원본 위치를 로컬에서 보관합니다. 서브에이전트 줄을 끼워 넣을 때도 발언 순번을 같이 계산하므로 문장이 같다는 이유로 다른 위치를 연결하지 않습니다. 실제 API 입력에는 파일 위치 대신 `[mN]` ID만 붙입니다. 긴 세션을 요약하는 프롬프트에도 ID와 짧은 원문 인용을 보존하도록 지시합니다.

긴 발언을 조각으로 나눌 때는 역할과 ID 길이도 추정 토큰 상한과 문자 상한에 포함합니다. 한 발언이 여러 조각으로 나뉘면 모든 부분에 같은 ID를 붙여 원래 위치를 유지합니다. 역할, ID와 최소 내용도 담을 수 없는 상한은 오류로 반환합니다. 토큰 수는 기존의 문자 수 기반 추정값이며 실제 공급자의 토큰 계산과는 다를 수 있습니다.

`Decision`, `TilItem`, `Correction`의 `evidence`는 `message_id`, `quote`, 로컬에서 연결한 `location`과 `role`을 담습니다. 공급자는 ID와 인용만 반환하며 Codex의 출력 스키마도 이 두 필드만 허용합니다. 공통 실행기는 모든 공급자의 분석 응답을 파싱한 뒤 같은 세션의 ID와 인용을 대조합니다. 인용은 원래 대화와 압축 또는 마스킹된 입력 양쪽에 연속된 부분 문자열로 있어야 합니다. 검사를 통과하지 못한 근거가 하나라도 있으면 해당 항목을 제외합니다. 결정과 TIL에는 사용자 근거, 정정에는 AI와 사용자 양쪽 근거를 요구합니다. `SourceMessage.message_index`로 인용한 모든 AI 발언이 모든 사용자 정정 발언보다 먼저 나왔는지 검사합니다. 원본 파일의 줄 번호나 인용 배열의 순서로 대화 순서를 추정하지 않습니다.

`Correction.kind`는 `factual_error`, `preference_change`, `unknown`입니다. 한국어와 영어 프롬프트가 사실 주장 정정과 선호 또는 접근 방식 변경을 구분하도록 지시합니다. 누락된 값과 알 수 없는 값은 `unknown`으로 읽으며, 정정 내용을 독립적으로 사실 확인했다는 의미로 쓰지 않습니다.

기존 캐시와 JSON 결과는 Serde 기본값으로 읽습니다. 누락된 `evidence`는 빈 배열이며 근거 미확인으로 표시합니다. 예전 결과를 새로 검증한 것처럼 승격하거나 캐시 버전 때문에 자동 재분석하지 않습니다. 새 결과에는 검사를 통과한 인용과 위치를 캐시하고 터미널과 Markdown에 표시합니다.

`rwd today`의 캐시 재사용 경로는 `prepare_cached_sources`에서 같은 입력 소스와 세션의 현재 발언을 찾고 `revalidate_cached_session`으로 근거를 다시 검사합니다. ID, 발언자, 인용, 파일 경로와 줄 번호가 모두 일치해야 위치를 유지합니다. 정정은 대화 순서도 검사합니다. 하나라도 불일치하면 해당 항목 전체의 위치와 발언자 표시를 지우고, 정정 종류도 `unknown`으로 바꿉니다. 원래 설명과 인용은 보존하며, 현재 위치로 추정해서 옮기거나 예전 미확인 근거에 위치를 새로 붙이지 않습니다. 이 검사는 현재 마스킹 설정도 적용합니다. 표시용 결과만 조정하므로 캐시 파일은 덮어쓰지 않으며, 근거 불일치 때문에 외부 AI를 다시 호출하지 않습니다.

검증 범위는 합성 로그의 위치 보존, 인용 대조, 분류 값과 기본값, 캐시 저장과 복원, 출력 표시입니다. 실제 AI의 추출과 분류 정확도는 별도로 측정해야 합니다. 인용의 존재를 검사하는 코드는 이유와 학습의 의미가 맞는지 판정하지 않습니다.

## 긴 세션의 후반 보존

`summarizer::split_into_chunks`는 입력 순서대로 발언을 나눕니다. `chunk_input_budgets`는 중간 요약 프롬프트와 원래 조각 번호를 위한 여유를 먼저 빼고, 호출당 추정 입력 30,000토큰과 `RateLimits.input_tokens_per_minute` 중 작은 값을 적용합니다. Codex는 `max_conversation_chars_with_prompt`가 반환한 문자 상한도 적용합니다. 역할과 원문 ID까지 포함해 나누므로 큰 발언 하나도 상한을 넘지 않습니다. 이 값은 애플리케이션의 추정 입력 상한이며 모델별 실제 토큰 계산이나 전역 할당량을 보장하지 않습니다.

`MAX_CHUNKS_PER_SESSION = 3`을 유지합니다. 세 조각 이하는 모두, 네 조각 이상은 첫 조각과 마지막 두 조각을 선택합니다. 중간 요약과 최종 조립 순서는 원래 조각 번호 순서입니다. 조각마다 원래 번호와 전체 수를 전달하며, 합친 입력에는 합성 요약 표제를 붙입니다. 한국어와 영어 프롬프트는 이전 선택, 변경과 마지막 상태를 구별하고 최종 정정의 대상과 근거를 함께 보존하도록 지시합니다. 원문 ID를 유지한 인용만 기존 `evidence::validate`에서 원래 발언과 위치에 연결할 수 있습니다. 합성 설명에 새 ID를 부여하지 않습니다.

각 조각의 `InputFragment`는 본문과 역할, 원래 발언 순번을 로컬에서 보관합니다. `ChunkSummary.source_fragments`에는 실제로 선택한 조각의 부분만 담습니다. `evidence::validate_summary`는 발언 순번으로 원문을 찾고, 인용이 같은 발언의 선택된 부분 하나 안에 있는지 확인한 뒤 기존 원문 대조와 정정 순서 검사를 적용합니다. 서로 떨어진 부분을 이어 인용을 만들지 않으며, 본문에 적힌 가짜 ID로 다른 발언의 위치를 빌리지 않습니다. 현재 마스킹 설정도 근거 대조에 적용합니다.

`ChunkSummary.coverage_notice`는 제외한 범위를 담습니다. 공통 실행기는 모델 응답의 근거를 검사한 뒤 이 안내를 `work_summary` 앞에 직접 붙입니다. 그래서 모델이 안내를 생략해도 터미널 결과, JSON 캐시와 Markdown에 남고 별도 저장 필드는 필요하지 않습니다. 분석 응답에 해당 세션이 없으면 오류를 반환하며, 누락 범위 표시가 없는 세 조각 이하의 요약과 직접 분석에도 같은 검사를 적용합니다. 기존 캐시의 범위를 추측해서 새 표시를 붙이거나 자동으로 외부 재분석하지 않습니다.

각 중간 요약은 최대 2,000토큰을 요청합니다. 마지막 중간 요약 뒤에도 최종 분석 호출을 위한 대기 시간을 둡니다. 공급자 오류나 빈 요약은 부분 성공으로 바꾸지 않고 반환합니다. 합친 요약은 최종 마스킹 뒤에 재시도 안내와 세션 표제를 포함한 입력 길이를 검사하고, 한도를 넘으면 오류를 냅니다. 기존 429 또는 JSON 오류 재시도는 한 번이며 실행 시도당 최대 세 번의 중간 요약과 한 번의 최종 분석을 반복할 수 있습니다.

직접 분석 여부도 재시도 안내를 포함한 실제 프롬프트의 길이로 정합니다. 모든 최종 분석 요청은 `prepare_analysis_input`에서 마스킹을 적용한 뒤 추정 토큰과 공급자 문자 상한을 다시 검사합니다. 마스킹 문구 때문에 입력이 길어져도 상한을 넘긴 채 호출하지 않습니다.

중간 구간은 여전히 빠질 수 있으며 부분 분석 표시에 해당 번호를 명시합니다. 실제 모델의 의미 보존은 합성 입력 기반 처리 범위 테스트와 별개입니다. 중간 요약 경로의 기존 마스킹 누락은 이 변경에서 해결하지 않습니다.

## Two-Stage Processing

1. **Parsing stage (rule-based)**: Transforms log files into structured data
   - Who said what (user / assistant)
   - Tool invocations
   - Error occurrences
   - Undo/correction patterns

2. **Analysis stage (LLM-based)**: Sends structured data to LLM for insight extraction
   - Uses structured data (not raw logs) to minimize information loss

## Input Sources

Each agent's logs are read by one source adapter in `src/sources/`. An adapter implements `SourceAdapter` (`id`, `display_name`, `collect`) and turns the logs for one local date into a `SourceCollection`:

- `roots`: log folders that exist on this machine (shown by `-v`, `--dry-run`, and `rwd doctor`)
- `sessions`: analysis-ready `SessionTask`s with activity range and optional token usage
- `excluded`: sessions dropped on purpose, by reason (`automated`, `subagent`, `print-mode`)
- `fingerprint`: session and entry counts, stored per adapter id in the cache to decide whether to re-analyze
- `verbose_lines`: extra diagnostics printed with `-v`

`registered_adapters()` returns the adapters in display order: Claude Code, Codex, Antigravity. `commands/`, the cache, and `doctor.rs` only walk this list and never call a source-specific parser. If one adapter fails, its name is printed as a warning and the other sources are still analyzed. Adding an agent means a new file in `src/sources/`, one line in `registered_adapters()`, and one settings struct in `config.rs`.

Dependency direction: `commands` and `doctor` → `sources` → `parser` and `analyzer`. `analyzer` does not know about `sources`.

### Claude Code

- Adapter: `sources/claude.rs` (id `claude`)
- Log location: `~/.claude/projects/` subdirectory JSONL files
- Format: Each line is an independent JSON object
- Excluded: automated sessions (SDK runs, hooks) unless `--include-automated` or `[input.claude] include_automated = true`
- Subagents: records in `<project>/<session>/subagents/agent-<agentId>.jsonl` (workflow ones in `subagents/workflows/<id>/`) are not sessions. The first user text and the last assistant text go into the parent prompt where the parent got the tool result with the same `agentId`, or at the record's first timestamp. Without a record file, the parent's `toolUseResult.prompt` and `content` are used

### Codex

- Adapter: `sources/codex.rs` (id `codex`)
- Log location: `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
- Format: Each line is `{"timestamp", "type", "payload"}` JSON object
- Entry types: session_meta, response_item, event_msg, turn_context
- Parser: Two-stage conversion (CodexRawEntry → CodexEntry)
- Prompt text: user lines hold only the typed text (`event_msg` `message`, or `input_text` blocks without app-injected context) and answer lines only the `output_text` blocks. Other payload strings (ids, `phase`, `status`, image references) are dropped. The `[META]` line keeps only `cwd`, `git.branch`, `git.repository_url`, `id`, `model_provider`, and a string `source` from `session_meta` (`SESSION_META_PROMPT_FIELDS`); `base_instructions` and account ids are dropped
- Excluded: sessions with explicit subagent metadata in `session_meta`
- Subagents: attached to the interactive parent by following `source.subagent.thread_spawn.parent_thread_id` (through other subagents if needed). Forked rollouts start with a copy of the parent's history, so the task is the first `agent_message` whose `recipient` is the subagent's `agent_path`, and the result is the last answer after it

### Subagent lines

`sources/subagent.rs` turns each parent session's subagent runs into `[SUBAGENT] <name>: <task>` and `[SUBAGENT_RESULT] <answer>` lines. Both texts go through `compact_log_like`, then tasks are capped at 2,000 characters each and 6,000 per session, and answers at 8,000 each and 24,000 per session; longer text keeps its first two thirds and last third around a `[... N자 생략 ...]` marker. The prompt builders in `analyzer/prompt.rs` take these as time-stamped lines (`TimedLine`) and put each before the first parent message stamped later, so `analyzer` still does not know about `sources`. Subagents never enter the session count or the cache fingerprint.

### Antigravity

- Adapter: `sources/antigravity.rs` (id `antigravity`), parser: `parser/antigravity.rs`
- Log location: `<brain>/<conversation id>/.system_generated/logs/transcript_full.jsonl` under these `brain` folders, in order: `[input.antigravity] roots`, `~/.gemini/antigravity-cli/brain` (agy CLI), `~/.gemini/antigravity/brain` (Antigravity app), `~/.gemini/antigravity-ide/brain` (Antigravity IDE), and the same folders under Windows homes in WSL
- Format: Each line is one conversation step with `step_index`, `type`, `created_at` (UTC), `content`, `thinking`, `tool_calls`. Missing or unknown fields and broken or half-written lines are skipped
- Read steps: `USER_INPUT` (only the text inside `<USER_REQUEST>`) and `PLANNER_RESPONSE` (thinking and answer as one `ASSISTANT` line, tool names as `TOOL` lines). Tool arguments, tool outputs, and every other step type are not sent to the LLM
- Session: one conversation folder; the same id under several folders is merged by `step_index`. Steps are filtered by `created_at`; files not modified since the target date start are not opened
- Excluded as `print-mode`: agy conversations whose id is missing from `~/.gemini/antigravity-cli/history.jsonl` and that have at most one user input (non-interactive `agy -p` runs). App and IDE folders are never excluded. Only `conversationId` is read from `history.jsonl`
- Not read: `transcript.jsonl` (truncated copy), `chunks/` (split copy), `conversations/*.db` and `*.pb`, and the credential and settings files under `~/.gemini`
- No token usage is recorded, so the info box shows no token totals for this source
- Settings: `[input.antigravity] enabled` (default `true`), `roots`, `include_print_mode` (default `false`)

## Project Structure

```
rwd/
├── Cargo.toml
├── CLAUDE.md
├── prompts/             # LLM prompt templates (EN/KO)
├── docs/
│   ├── ARCHITECTURE.md  # This document
│   ├── MILESTONES.md
│   └── CONVENTIONS.md
├── src/
│   ├── main.rs          # CLI 진입점과 명령 분기
│   ├── cli.rs           # clap-based CLI definitions
│   ├── commands/        # 명령 실행 흐름
│   │   ├── mod.rs       # 날짜와 언어 선택, 명령 진입점
│   │   ├── today.rs     # 일일 로그 수집, 분석, 캐시와 저장
│   │   ├── summary.rs   # 요약과 Slack용 글 생성, 클립보드 복사
│   │   ├── collection.rs# 등록된 어댑터를 차례로 불러 결과와 실패를 모음
│   │   ├── display.rs   # 정보 상자, --dry-run 세션 목록 출력
│   │   └── background.rs# 백그라운드 프로세스, 잠금과 알림
│   ├── sources/         # 에이전트별 로그 어댑터 (SourceAdapter)
│   │   ├── mod.rs       # 어댑터 계약, 공통 타입, registered_adapters
│   │   ├── claude.rs    # Claude Code 세션 수집, subagents/ 기록 붙이기
│   │   ├── codex.rs     # Codex 세션 수집, 서브에이전트를 부모 세션에 붙이기
│   │   ├── subagent.rs  # 서브에이전트 줄 만들기와 길이 상한
│   │   └── antigravity.rs # Antigravity 대화 수집, agy -p 실행 빼기
│   ├── doctor.rs        # 설치, 설정과 로그 폴더 진단
│   ├── messages.rs      # Centralized user-facing strings
│   ├── config.rs        # 설정 타입, 파일 저장, 이전 형식 변환과 값 해석
│   ├── config/
│   │   ├── commands.rs  # 초기 설정과 명령으로 설정값 변경
│   │   ├── interactive.rs # 대화형 설정 메뉴와 입력 취소
│   │   ├── auth.rs      # 인증 상태 확인, 키 검증과 마스킹
│   │   ├── vault.rs     # Obsidian 경로 탐색과 WSL 경로 처리
│   │   └── reset.rs     # 설정, 캐시와 worker 상태 초기화
│   ├── parser/          # Log parsing modules
│   │   ├── mod.rs
│   │   ├── claude.rs    # Claude Code log parser
│   │   ├── codex.rs     # Codex log parser
│   │   ├── antigravity.rs # Antigravity transcript_full.jsonl and agy history.jsonl reader
│   │   └── roots.rs     # Root dedupe and WSL Windows home discovery
│   ├── analyzer/        # Structured data → LLM insight extraction
│   │   ├── mod.rs       # 세션 작업 생성, 실행계획과 분석 진입점
│   │   ├── executor.rs  # 병렬 실행, 재시도와 결과 순서 복원
│   │   ├── anthropic.rs # Anthropic Claude API client
│   │   ├── codex_exec.rs# Codex CLI (codex exec) provider backend
│   │   ├── insight.rs   # Response parsing
│   │   ├── openai.rs    # OpenAI API client
│   │   ├── planner.rs   # Execution plan generation
│   │   ├── prompt.rs    # Prompt construction
│   │   ├── provider.rs  # LlmProvider enum, provider selection
│   │   └── summarizer.rs# Chunk summarization
│   ├── output/          # Markdown generation and file saving
│   │   ├── mod.rs
│   │   └── markdown.rs
│   ├── redactor/        # Sensitive data masking
│   │   ├── mod.rs       # Public API: redact_text(), RedactResult
│   │   └── patterns.rs  # Built-in patterns (LazyLock initialized)
│   ├── cache.rs         # Analysis result caching
│   └── update.rs        # Self-update via GitHub Releases
└── tests/
```

## Core Dependencies

| Crate        | Purpose              |
| ------------ | -------------------- |
| `clap`       | CLI parsing          |
| `regex`      | Regex matching       |
| `serde`      | Serialization        |
| `serde_json` | JSON/JSONL parsing   |
| `reqwest`    | HTTP client          |
| `tokio`      | Async runtime        |
| `chrono`     | Date/time handling   |
| `dialoguer`  | Interactive prompts  |
| `notify-rust`| OS notifications     |
