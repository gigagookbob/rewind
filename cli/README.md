# rwd (rewind)

CLI that turns your AI agent sessions into a daily journal. Extracts decisions, learnings, and corrections, then saves them as Obsidian Daily Notes.

Reads local session logs from Claude Code, Codex, and Antigravity (agy CLI, Antigravity app, Antigravity IDE).

## rwd vs Claude Code `/insights`

Claude Code has a built-in `/insights` command. rwd serves a different purpose.

`/insights` answers **"Am I using Claude efficiently?"** — it analyzes 30 days of tool usage patterns, token consumption, and friction points as an HTML dashboard. It's a coding efficiency report.

rwd answers **"What did I decide today, and why?"** — it extracts decisions, curiosities, and model corrections from all sessions and accumulates them as Obsidian Daily Notes. It's a developer journal.

| | `/insights` | `rwd` |
|--|-------------|-------|
| Focus | Tool usage patterns, efficiency | Decisions, learning, model corrections |
| Period | 30-day rolling | Daily |
| Output | HTML dashboard (one-off) | Obsidian Daily Notes (cumulative) |
| Analysis | Quantitative (tokens, tool counts, code volume) | Qualitative (why A over B, what was confusing) |
| Environment | Inside Claude Code session only | Standalone CLI, runs anywhere |

## Installation

### One-line install (macOS / Linux / WSL2)

```bash
curl -fsSL https://raw.githubusercontent.com/gigagookbob/rewind/main/cli/install.sh | bash
```

By default, this installs `rwd` to `~/.local/bin` (override with `RWD_INSTALL_DIR`).

If you previously installed via `cargo install` or `/usr/local/bin`, keep only one binary in PATH:

```bash
cargo uninstall rwd
sudo rm /usr/local/bin/rwd
```

Native Windows is not supported.

If you're on Windows, install WSL2 first, open a Linux shell (for example Ubuntu), then run:

```bash
curl -fsSL https://raw.githubusercontent.com/gigagookbob/rewind/main/cli/install.sh | bash
```

### Build from source

```bash
cargo install --git https://github.com/gigagookbob/rewind.git
```

### Prebuilt binary targets

- `aarch64-apple-darwin` (macOS Apple Silicon)
- `x86_64-unknown-linux-gnu` (Linux x86_64)

> If macOS shows "unidentified developer" warning:
> ```bash
> xattr -d com.apple.quarantine ~/.local/bin/rwd
> ```

## Setup

```bash
rwd init
```

Sets up your LLM provider, credentials, output path, and language preference.
If you choose `codex`, `rwd` uses your existing `codex login` session (no API key required).
Obsidian vault is auto-detected if available.
Config is stored at `~/.config/rwd/config.toml`.

Auth methods by provider:
- `anthropic`, `openai`: API key auth
- `codex`: Codex login session auth (`codex login`)

### Change settings

`rwd config`의 대화형 메뉴에서 API 키를 입력하거나 붙여넣으면 원문 대신 `*`가 표시됩니다. Enter로 입력을 마치고 Esc로 취소할 수 있습니다.

```bash
rwd config output-path /path/to/vault    # Change output path
rwd config provider codex                # Change LLM provider
rwd config api-key sk-...                # Set API key for current provider (openai/anthropic)
rwd config openai-api-key sk-...         # Set OpenAI API key directly
rwd config anthropic-api-key sk-ant-...  # Set Anthropic API key directly
rwd config anthropic-workspace-id wrkspc_... # Select workspace for an organization-scoped key
rwd config codex-model default           # Use the model configured in Codex CLI
rwd config codex-reasoning xhigh         # Override Codex reasoning (default: xhigh)
rwd config claude-include-automated true # Include automated Claude sessions by default
rwd config lang ko                       # Change output language (en/ko)
rwd auth status                          # Show provider auth method + credential state
rwd reset --dry-run                      # Preview reset targets
rwd reset --yes                          # Reset local config/cache state
```

### Anthropic 조직 범위 API 키

조직 범위 키를 사용하려면 [Claude Console의 워크스페이스 설정](https://platform.claude.com/settings/workspaces)에서 사용할 워크스페이스의 ID를 확인한 뒤 등록합니다.

```bash
rwd config anthropic-workspace-id wrkspc_실제ID
rwd auth status
```

`wrkspc_실제ID`는 복사한 ID로 바꿉니다. `rwd config`의 대화형 메뉴에서도 설정할 수 있습니다. 이 값은 `~/.config/rwd/config.toml`의 `[llm]` 아래 `anthropic_workspace_id`에 저장하며, 분석, 요약, Slack 문구 생성, 호출 한도 확인과 API 키 확인에 함께 사용합니다.

특정 워크스페이스에 연결된 키는 ID를 따로 설정하지 않아도 됩니다. 키를 바꿀 때 기존 설정을 지우려면 다음 명령을 사용합니다.

```bash
rwd config anthropic-workspace-id ""
```

워크스페이스 ID가 빠졌거나 크레딧이 부족한 경우에는 해당 원인과 조치 방법을 표시합니다. 인증 실패, 권한 부족, 호출 한도 초과도 구분합니다. 사전 호출에서 인증이나 설정 오류가 확인되면 세션 분석을 시작하지 않습니다. `rwd auth status`는 저장된 설정을 보여 주며, Anthropic API 연결을 검사하지는 않습니다.

오류 응답에는 대화 내용이나 비밀값이 섞일 수 있어 원문을 출력하지 않습니다. 식별한 원인은 정해진 안내 문구로 표시하고, 알 수 없는 오류는 HTTP 상태와 확인 가능한 오류 종류, 요청 ID를 표시합니다. 오류 본문은 최대 16 KiB, 최대 5초 동안만 읽습니다.

요청에 워크스페이스 ID를 지정하는 규칙은 [Anthropic 인증 문서](https://platform.claude.com/docs/en/manage-claude/authentication)를 따릅니다.

### API keys via environment variables

`openai_api_key` and `anthropic_api_key` in `~/.config/rwd/config.toml`
support `${VAR}` placeholders, expanded from the process environment at
runtime. Use this to keep secrets out of the file (and out of backups
like Time Machine / iCloud):

```toml
[llm]
provider = "openai"
openai_api_key = "${OPENAI_API_KEY}"
```

If the referenced variable is not set when `rwd` runs, the command fails
with a clear error naming the missing variable. Literal keys (those
without a `${...}` placeholder) keep working unchanged.

### Sensitive data masking

`today`의 직접 분석, 긴 세션의 모든 중간 요약과 최종 분석, `summary`, `slack`과 재시도에서 내장 규칙으로 감지한 API 키, 토큰, 개인키와 사설 IP 등을 가립니다. 메시지를 분할하기 전에 가리고, 과거 참고 발언도 온전한 원문을 가린 뒤 기존 길이 예산으로 다시 고릅니다. 캐시와 중간 요약을 포함한 입력은 공급자 전송 직전에 다시 검사합니다. HTTP 인증 헤더에 사용하는 자격증명은 그대로 유지합니다.

마스킹은 기본 활성화입니다. 설정을 읽지 못하면 AI 전송을 멈춥니다. 보호를 명시적으로 끄려면 `~/.config/rwd/config.toml`에 다음 설정을 추가합니다. 이 경우 대화와 캐시의 감지 대상 값도 가리지 않고 전송합니다.

```toml
[redactor]
enabled = false
```

전송할 프롬프트와 표제를 포함해 문자 수와 예상 토큰 예산을 검사합니다. 마스킹으로 입력이 길어져 제한을 넘으면 오류로 반환하고 뒤를 자르지 않습니다. 화면의 마스킹 수는 원문에서 탐지한 횟수이며, 같은 원문의 재시도와 파생 결과 재검사로 중복 집계하지 않습니다.

로그 압축과 서브에이전트의 중간 생략이 감지 대상의 접두사만 없애지 않도록 해당 입력은 공통 마스킹 경계까지 온전하게 유지합니다. 마스킹 후 남은 로그를 압축합니다. 해당 서브에이전트 입력의 부분별 사전 제한은 보류되지만, 최종 공급자 입력과 각 요약 조각의 예산은 계속 검사합니다. 비활성화 설정이면 값은 그대로 남으며 입력이 커져 중간 요약이나 예산 오류로 이어질 수 있습니다.

인용은 원문과 보호된 입력에 모두 존재해야 합니다. 비밀값이 포함된 인용이나 마스킹 표식으로 바뀐 인용은 근거로 인정하지 않으며 원래 비밀값을 복원하지 않습니다. 기존 캐시는 전송 전에 보호하지만 파일 자체를 일괄 수정하지 않습니다.

탐지 규칙은 임의의 모든 비밀값을 보장하지 않습니다. 예를 들어 환경 변수 형태의 값은 기존 규칙대로 따옴표로 감싼 `password`, `secret`, `api_key` 할당을 검사합니다. 메시지를 이어 탐지하므로 인접한 일반 문장이 더 가려질 수 있고, 끝 표시 없는 개인키는 남은 입력을 보수적으로 가립니다. 공급자 오류 본문, JSON 파싱 실패의 응답 미리보기와 Codex 실패 출력은 오류에 그대로 싣지 않습니다.

## Usage

```bash
rwd today              # Analyze today's AI sessions → save to Obsidian Daily Notes
rwd today --lang ko    # Override output language for this run
rwd today --include-automated  # Include automated Claude sessions for this run
rwd today --dry-run    # List the sessions that would be analyzed, without calling the LLM
rwd summary            # Generate progress summary (Markdown) → save + copy to clipboard
rwd slack              # Generate Slack-ready message → copy to clipboard
rwd config             # Change settings (interactive menu)
rwd auth status        # Show current provider auth status
rwd reset --dry-run    # Preview files that reset will remove
rwd doctor             # Diagnose PATH/install/config/session-root issues
rwd update             # Update to the latest version
```

Update notice env toggles:
- `RWD_DISABLE_UPDATE_CHECK=1 rwd today` — disable update notice
- `RWD_FORCE_UPDATE_CHECK=1 cargo run -- today` — force update check even in local dev build

### 프로젝트와 질문 및 후속 작업

`rwd today`는 프로젝트 아래에 여러 AI 도구의 세션을 모으고, 각 세션의 작업 제목과 소스 이름, 전체 세션 ID를 보여 줍니다. 프로젝트는 로그에 명시된 절대 작업 경로로 구분합니다. 폴더 이름이 같아도 전체 경로가 다르면 별도 그룹이며, 경로나 제목을 확인할 근거가 없으면 미확인으로 표시합니다. 현재 Git 정보를 조사해 과거 프로젝트를 추정하지 않습니다. 하위 디렉터리와 별도 체크아웃은 합치지 않습니다. 자세한 식별 정책은 [회고 명세](SPEC-retrospective.md)에 있습니다.

질문에는 원래 발언과 이후 답변, 사용자의 해결 확인 또는 미해결 명시를 시간순으로 연결합니다. AI가 답변했다는 사실만으로 사용자가 이해하거나 문제가 해결됐다고 표시하지 않습니다. 사용자의 마지막 해결 또는 미해결 확인은 이후 AI 설명만으로 바뀌지 않습니다. 후속 작업은 사용자 요청과 AI 제안을 구별하고 진행 요청, 완료 보고와 철회 발언을 함께 표시합니다. 완료 보고는 실제 실행을 독립적으로 검증했다는 뜻이 아닙니다. 담당자를 추정하지 않습니다.

과거 참고에만 있는 질문이나 작업을 오늘 새 항목으로 반복하지 않습니다. 과거 질문에 오늘 답변하거나 과거 작업을 오늘 완료 또는 철회했다면 두 날짜의 근거를 연결합니다. 기존 캐시의 `curiosities`는 상태와 근거 미확인으로 남습니다. 새 구조가 없는 캐시를 임의로 미해결이나 완료 상태로 바꾸지 않습니다.

표시는 대화에서 확인한 상태를 읽는 용도입니다. 체크박스를 제공하지 않으며 직접 추가하는 메모는 자동 생성 영역 밖에 씁니다. 기존 자동 영역을 편집하면 다음 저장은 충돌 오류로 멈추고 사용자의 편집을 보존합니다. `summary`와 `slack`은 기존 작업 공유 역할을 유지하며 질문과 후속 작업을 자동으로 덧붙이지 않습니다. 공유 입력에는 프로젝트 경로 대신 실행 내 구분 키를 사용하고 같은 이름의 다른 프로젝트를 구별합니다.

원문 존재, 발언자, 순서와 선택된 입력 범위는 코드로 대조합니다. 인용이 해당 질문의 실제 답인지, 사용자가 해결이나 철회를 뜻했는지에 대한 의미 판정은 AI 추출에 의존하며 완전히 검증하지 못합니다. 생략된 이력을 복구하거나 모든 날짜의 작업 상태를 추적하지 않습니다.

### 회고의 근거와 정정 분류

`rwd today`의 새 분석은 결정과 TIL에 원문 발언, 발언자, 로컬 로그의 `파일 경로:줄 번호`를 표시합니다. 정정에도 같은 근거가 붙습니다. 줄 번호는 빈 줄과 파싱하지 못한 줄까지 센 JSONL의 실제 위치이며, 인용은 그 줄에서 추출한 대화 텍스트의 일부입니다. 대화가 여러 텍스트 블록으로 나뉘면 해당 블록들을 담은 줄을 가리킵니다.

AI는 입력에 붙인 `m1` 같은 발언 ID와 원문 인용만 반환합니다. `rwd`는 인용이 원래 발언과 분석용 텍스트 양쪽에 있는지 검사하고, 파일 위치와 발언자는 로컬에서 연결합니다. ID는 분석 입력 안에서만 쓰며 다음 실행에서도 같은 값이라고 보장하지 않습니다. 위치 연결을 위한 로컬 경로는 AI에게 보내는 메타 정보에 추가하지 않습니다.

- 결정은 사용자 발언 근거가 필요합니다. 대화에 명시된 이유만 쓰고, 이유가 없으면 비워 둡니다.
- TIL도 사용자가 실제로 이해하거나 적용했다는 발언 근거가 필요합니다. AI가 설명했다는 이유만으로 학습을 확정하지 않습니다.
- 새 분석에서 근거가 없거나 인용이 틀린 항목은 제외합니다. 내부 추론, 도구 출력, 메타 정보와 붙여 넣은 서브에이전트 요약은 근거로 인정하지 않습니다. 긴 세션의 중간 요약에서 ID나 인용을 잃은 경우에도 해당 항목이 빠질 수 있습니다.
- 정정은 `factual_error`(사실 오류 정정), `preference_change`(선호 또는 접근 방식 변경), `unknown`(분류 미확인)으로 표시합니다. 복잡도나 취향 때문에 제안을 거절한 경우를 사실 오류로 분류하지 않도록 지시하며, 모호하거나 두 경우가 섞이면 `unknown`으로 남깁니다. 근거에는 정정 대상 AI 발언과 그 뒤의 사용자 정정 발언만 넣습니다. 대화 순서가 뒤집히거나 정정 뒤의 AI 확인 응답이 섞인 새 항목은 제외합니다.

검사는 인용의 원문 일치와 위치를 확인합니다. 인용이 설명을 충분히 뒷받침하는지, 사용자가 실제로 학습했는지, 정정된 기술적 주장이 사실인지는 AI 해석에 남아 있습니다. 터미널과 일일 노트에도 이 한계를 표시합니다.

기존 캐시는 그대로 읽습니다. 새 필드가 없는 결정과 TIL은 근거 미확인으로, 기존 정정은 분류 미확인으로 표시하며 근거를 소급해서 만들지 않습니다.

`rwd today`는 캐시를 표시하기 전에 현재 수집한 같은 입력 소스와 세션의 발언 ID, 발언자, 파일 경로, 줄 번호와 인용을 대조합니다. 하나라도 맞지 않으면 해당 항목의 모든 근거를 미확인으로 표시하고, 정정은 분류도 미확인으로 바꿉니다. 정정의 대화 순서도 다시 검사합니다. 근거 재검사는 로컬에서 끝나며 표시용 결과만 조정합니다. 분석 입력이 바뀌면 아래의 여러 날 세션 캐시 정책에 따라 재분석합니다.

기존 날짜를 새 형식으로 분석하려면 기존 옵션인 `rwd today --date YYYY-MM-DD --no-cache`를 사용할 수 있습니다. 이 명령은 설정된 외부 AI를 호출합니다.

### 긴 세션의 분석 범위

입력 제한을 넘는 세션은 중간 요약을 거칩니다. 세 조각 이하는 모두 처리합니다. 네 조각 이상이면 첫 조각과 연속된 마지막 두 조각을 원래 순서대로 처리합니다. 마지막 발언만 떼지 않고 후반 맥락을 함께 보내며, 한국어와 영어 프롬프트가 결정 변경, 실패 해결과 마지막 사용자 정정을 우선 보존하도록 지시합니다.

중간 조각은 빠질 수 있습니다. 예를 들어 여덟 조각이면 1, 7, 8번을 보내고 2..=6번은 제외합니다. 이 범위와 전체 조각 수를 터미널과 분석 입력에 알리고, 모델 응답 뒤에도 작업 요약 앞에 부분 분석 표시를 붙입니다. 표시는 JSON 캐시와 일일 Markdown에 남습니다. 제외 구간에만 있는 결정이나 정정은 확인하지 못하므로 전체 세션을 분석한 결과로 읽으면 안 됩니다.

중간 요약 호출은 실행 시도당 최대 세 번이며 각 출력 요청 상한은 2,000토큰입니다. 입력은 호출당 추정 30,000토큰과 공급자의 분당 입력 한도 중 작은 값 안에서 프롬프트, 조각 번호, 역할과 발언 ID를 포함합니다. Codex는 기존 문자 상한도 함께 적용합니다. 긴 발언은 나눈 모든 부분에 원래 ID를 유지합니다. 토큰 수는 문자 수로 추정하므로 공급자의 실제 계산과 다를 수 있습니다.

합친 요약이 최종 분석의 입력 제한을 넘거나 요약이 비어 있으면 내용을 잘라 성공으로 처리하지 않고 오류를 반환합니다. 기존 재시도는 최대 한 번이므로 재시도까지 발생하면 중간 요약은 최대 여섯 번, 최종 분석은 최대 두 번 호출할 수 있습니다. 짧은 세션은 기존처럼 직접 분석합니다. 이전 맥락 정책이 없는 구 캐시는 다음 분석 명령에서 한 번 갱신합니다. 그 뒤에도 강제로 다시 분석하려면 `--no-cache`를 사용합니다.

원문 ID와 인용을 중간 요약에서 잃으면 해당 근거가 필요한 항목은 제외될 수 있습니다. 로컬 테스트는 처리 범위, 순서와 원문 대조를 확인하며 실제 모델이 결론을 빠짐없이 요약하는지는 보장하지 않습니다. 중간 요약에도 직접 분석과 같은 마스킹 정책을 적용합니다.

요약을 거친 근거는 실제로 선택한 조각 안의 같은 발언 부분에도 인용이 있어야 합니다. 같은 긴 발언의 제외된 부분을 인용하거나, 다른 발언에 적힌 ID를 빌려 원문 위치를 붙이지 않습니다. 직접 분석도 재시도 안내와 마스킹을 포함한 최종 입력 길이를 검사하며, 응답에 해당 세션이 없으면 오류를 반환합니다.

### 여러 날 이어지는 세션

Codex는 시작 날짜 폴더에 그대로 남아 있는 세션도 찾아 당일 재개한 발언을 수집합니다. 실제 발언 시각으로 날짜를 나누며, 당일 활동이 없는 세션과 대상 날짜 이후 발언은 포함하지 않습니다. 과거 폴더를 모두 읽으므로 로그가 많이 쌓이면 수집 시간이 늘어날 수 있습니다.

Claude Code, Codex와 Antigravity는 같은 세션의 이전 사용자와 답변 중 최근 12개, 참고 표시를 포함해 합계 4,000자 이내를 분석에 추가합니다. 공간이 부족한 발언은 끝부분을 남기며, 더 오래된 이유나 결정을 놓칠 수 있습니다. 참고 시각은 UTC로 표시합니다. 결정, 학습과 정정에는 당일 사용자 발언 근거를 요구하고 과거 선택을 오늘의 새 결정으로 반복하지 않도록 지시합니다. 과거 답변을 오늘 정정했다면 두 근거의 시각과 참고 구분을 출력합니다. 의미 해석과 최종 선택의 정확도는 모델에 의존합니다.

세션 수, 미리보기의 발언 수와 활동 시각, Claude 사용량은 당일 자료만 셉니다. 예상 분석 토큰에는 이전 참고가 포함됩니다. 긴 입력은 기존 첫 조각과 마지막 두 조각을 선택하므로 당일 최종 결론을 우선 보존합니다. 입력 한도가 작으면 이전 참고와 초반 당일 내용 일부가 빠질 수 있으며, 기존 부분 분석 표시가 남습니다.

`today`, `summary`와 `slack`은 현재 분석 메시지와 근거 위치, 시각의 버전 해시를 비교합니다. 이 정책이 없는 구 캐시와 선택한 참고 내용이 바뀐 캐시는 다음 명령 실행 때 재분석하며 설정된 외부 AI를 호출합니다. 미래 발언이나 선택 범위 밖의 과거 내용만 추가된 경우에는 재분석하지 않습니다. 캐시 파일 형식과 일일 노트 저장 위치는 유지합니다.

### 일일 노트와 수동 메모 보존

`today`와 `summary`는 날짜 파일 전체나 제목으로 찾은 구간을 덮어쓰지 않습니다. 각각 `analysis`와 `summary`라는 자동 영역을 HTML 주석으로 구분하고, 본문이 마지막 저장 그대로일 때만 해당 영역을 갱신합니다. 새 분석, `--no-cache` 재분석과 캐시 재출력에 같은 정책을 적용합니다. 자동 영역 밖의 메모, 문서 제목, 앞머리 정보와 다른 결과는 공백과 줄바꿈까지 보존합니다.

표시 없는 기존 일일 노트와 예전 버전의 분석은 전부 보존하고 새 자동 영역을 뒤에 추가합니다. 첫 실행에서는 예전 결과와 새 영역이 함께 보일 수 있습니다. 이후 정상 반복 실행은 같은 영역만 갱신합니다. 같은 제목의 사용자 기록과 코드 상자 안의 표시 예시는 자동 결과로 간주하지 않습니다.

자동 영역 안에 메모를 추가하거나 본문을 편집하면 체크섬이 달라져 저장을 중단합니다. 표시가 손상 또는 중복됐거나 닫히지 않은 코드 상자처럼 경계가 불분명한 파일도 변경하지 않고 오류를 반환합니다. 두 자동 영역 중 어느 쪽이든 충돌하면 전체 저장을 멈춥니다. 메모는 영역 밖에 작성하는 것이 좋습니다. 충돌 시에는 메모를 밖으로 옮기고 원래 본문을 복원하거나, 해당 영역의 시작과 끝 주석만 모두 제거해 기존 내용을 수동 기록으로 남긴 뒤 다시 실행할 수 있습니다. 체크섬은 우발적인 편집 감지용이며 악의적인 변경을 검증하지는 않습니다.

날짜별 숨김 파일 `.YYYY-MM-DD.rwd.lock`에 잠금을 잡아 여러 rwd의 동시 저장을 조정합니다. 잠금 파일은 비어 있으며 실행 후에도 유지됩니다. 저장 중 잠금 파일을 삭제하면 안 됩니다. 잠금이 겹치면 오류를 반환하므로 다른 실행이 끝난 뒤 다시 시도합니다. 같은 폴더의 임시 파일에 전부 쓰고 동기화한 뒤 교체하므로 쓰기 실패로 기존 날짜 파일이 잘리지 않습니다. 기존 파일의 일반 권한은 복사하며 심볼릭 링크에는 저장하지 않습니다. 강제 종료로 남은 `YYYY-MM-DD.rwd-프로세스번호-일련번호.tmp`는 다음 저장에서 덮어쓰지 않습니다.

교체 직전에 외부 편집도 다시 확인하지만, 외부 편집기는 잠금에 참여하지 않으므로 마지막 확인과 교체 사이의 편집까지 보장하지 못합니다. Obsidian 편집과 동기화가 끝난 뒤 명령을 실행하는 것이 좋습니다. 동기화 서비스의 충돌과 전원 장애 뒤의 디렉터리 내구성은 보장하지 않습니다. 저장 오류는 명령 실패로 전달하며 백그라운드 실행도 완료로 알리지 않습니다.

### Preview with `--dry-run`

`rwd today --dry-run` collects sessions and prints, per source, the log folder, each session (short id, time range, user and assistant message counts, estimated tokens), and how many sessions were excluded and why. It then exits.

- It does not load the LLM provider, call the LLM, read or write the cache, or write Markdown, so it costs nothing and no conversation leaves your machine.
- It works without a config file; only the default log folders are checked then.
- It can be combined with `--date YYYY-MM-DD`, `--include-automated`, and `-v` (adds each source's diagnostics).
- Conversation text is never printed.

### Input sources

Claude session filtering:
- Default behavior excludes automated Claude sessions from `rwd today`.
- Subagents never count as sessions. What each subagent was asked and its final answer are added to the parent session's prompt, read from `<session>/subagents/agent-*.jsonl` (or from the parent's tool result when that file is missing). Its tool calls and intermediate turns are not sent.
- Enable globally with:
```toml
[input.claude]
include_automated = true
```
- Keep custom Claude roots in either legacy `[input] claude_roots = [...]` or new `[input.claude] roots = [...]`.

Codex session filtering:
- Sessions with explicit subagent metadata in `session_meta` never count as sessions.
- Hard signals are `source.subagent`, `agent_role`, and `agent_nickname`.
- A spawned subagent's task (the first message sent to it) and final answer are added to its interactive parent's prompt, found through `parent_thread_id`. Subagents with no parent session that day are only counted in `-v`.
- Subagent text is capped for both sources: tasks at 2,000 characters each and 6,000 per session, final answers at 8,000 each and 24,000 per session. Longer text keeps its first two thirds and last third.
- Worktree interactive sessions are still included by default.
- `cwd` patterns and prompt heuristics are not used.

Codex Desktop user `response_item` / `input_text` blocks are also supported. App-injected context is excluded, and legacy `event_msg` echoes are counted once.

Codex messages send only the text itself: the typed `message` or `input_text` for user turns and the `output_text` blocks for answers. Message ids, `phase`/`status` values, and image or file references are not sent. The session's `[META]` line keeps only the working directory, git branch and repository URL, session id, model provider, and source; Codex's built-in instructions and account ids are not sent. Cached inputs are checked against the multi-day context policy described above; `rwd today --no-cache --date <date>` forces a fresh analysis.

Antigravity sessions:
- Read by default from `~/.gemini/antigravity-cli/brain` (agy CLI), `~/.gemini/antigravity/brain` (Antigravity app), and `~/.gemini/antigravity-ide/brain` (Antigravity IDE). In WSL, the same folders under Windows homes are also checked.
- Each conversation's `.system_generated/logs/transcript_full.jsonl` is the only file read in these folders. One conversation is one session.
- Only your typed request, the model's thinking and answers, and tool names are sent to the LLM. Tool arguments, tool outputs, and app-added metadata are not.
- Non-interactive `agy -p` runs are skipped by default: agy conversations whose id is not in `~/.gemini/antigravity-cli/history.jsonl` and that have at most one prompt. Only conversation ids are read from that file. App and IDE conversations are never skipped this way.
- Token usage is not recorded for Antigravity, so no token totals are shown for it.
- Settings (all optional):
```toml
[input.antigravity]
enabled = true              # set to false to keep Antigravity out of rwd
roots = []                  # extra brain folders, searched before the defaults
include_print_mode = false  # set to true to also analyze agy -p runs
```
- `rwd doctor` and `rwd today -v` show which Antigravity folders were found.

## Release policy (maintainers)

- Release is triggered automatically on `main` push only when these files changed:
  - `cli/src/**`, `cli/prompts/**`, `cli/Cargo.toml`, `cli/Cargo.lock`
  - `.github/workflows/release-on-main.yml`, `.github/workflows/release.yml`
- Release tag is derived from `cli/Cargo.toml` package version (`vX.Y.Z`).
- If the tag already exists, release is skipped.
- Docs-only changes (for example `README.md`, `docs/**`) do not trigger release.
- Release notes are published in two layers: a short custom summary for `rwd`, then GitHub's full categorized changelog.
- Maintainer tone and workflow guide: [docs/RELEASES.md](docs/RELEASES.md)

## Uninstall

```bash
# Remove rwd binary
sh -c 'rm "$(command -v rwd)"'

# Also remove config/data:
rm -rf ~/.config/rwd ~/.rwd
```

## License

MIT
