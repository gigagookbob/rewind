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

```bash
rwd config output-path /path/to/vault    # Change output path
rwd config provider codex                # Change LLM provider
rwd config api-key sk-...                # Set API key for current provider (openai/anthropic)
rwd config openai-api-key sk-...         # Set OpenAI API key directly
rwd config anthropic-api-key sk-ant-...  # Set Anthropic API key directly
rwd config codex-model default           # Use the model configured in Codex CLI
rwd config codex-reasoning xhigh         # Override Codex reasoning (default: xhigh)
rwd config claude-include-automated true # Include automated Claude sessions by default
rwd config lang ko                       # Change output language (en/ko)
rwd auth status                          # Show provider auth method + credential state
rwd reset --dry-run                      # Preview reset targets
rwd reset --yes                          # Reset local config/cache state
```

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

When running `rwd today`, sensitive data in session logs (API keys, tokens, private IPs, etc.) is automatically masked before sending to the LLM. Enabled by default. To disable, add to `~/.config/rwd/config.toml`:

```toml
[redactor]
enabled = false
```

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

### 회고의 근거와 정정 분류

`rwd today`의 새 분석은 결정과 TIL에 원문 발언, 발언자, 로컬 로그의 `파일 경로:줄 번호`를 표시합니다. 정정에도 같은 근거가 붙습니다. 줄 번호는 빈 줄과 파싱하지 못한 줄까지 센 JSONL의 실제 위치이며, 인용은 그 줄에서 추출한 대화 텍스트의 일부입니다. 대화가 여러 텍스트 블록으로 나뉘면 해당 블록들을 담은 줄을 가리킵니다.

AI는 입력에 붙인 `m1` 같은 발언 ID와 원문 인용만 반환합니다. `rwd`는 인용이 원래 발언과 분석용 텍스트 양쪽에 있는지 검사하고, 파일 위치와 발언자는 로컬에서 연결합니다. ID는 분석 입력 안에서만 쓰며 다음 실행에서도 같은 값이라고 보장하지 않습니다. 위치 연결을 위한 로컬 경로는 AI에게 보내는 메타 정보에 추가하지 않습니다.

- 결정은 사용자 발언 근거가 필요합니다. 대화에 명시된 이유만 쓰고, 이유가 없으면 비워 둡니다.
- TIL도 사용자가 실제로 이해하거나 적용했다는 발언 근거가 필요합니다. AI가 설명했다는 이유만으로 학습을 확정하지 않습니다.
- 새 분석에서 근거가 없거나 인용이 틀린 항목은 제외합니다. 내부 추론, 도구 출력, 메타 정보와 붙여 넣은 서브에이전트 요약은 근거로 인정하지 않습니다. 긴 세션의 중간 요약에서 ID나 인용을 잃은 경우에도 해당 항목이 빠질 수 있습니다.
- 정정은 `factual_error`(사실 오류 정정), `preference_change`(선호 또는 접근 방식 변경), `unknown`(분류 미확인)으로 표시합니다. 복잡도나 취향 때문에 제안을 거절한 경우를 사실 오류로 분류하지 않도록 지시하며, 모호하거나 두 경우가 섞이면 `unknown`으로 남깁니다. 근거에는 정정 대상 AI 발언과 그 뒤의 사용자 정정 발언만 넣습니다. 대화 순서가 뒤집히거나 정정 뒤의 AI 확인 응답이 섞인 새 항목은 제외합니다.

검사는 인용의 원문 일치와 위치를 확인합니다. 인용이 설명을 충분히 뒷받침하는지, 사용자가 실제로 학습했는지, 정정된 기술적 주장이 사실인지는 AI 해석에 남아 있습니다. 터미널과 일일 노트에도 이 한계를 표시합니다.

기존 캐시는 그대로 읽습니다. 새 필드가 없는 결정과 TIL은 근거 미확인으로, 기존 정정은 분류 미확인으로 표시하며 근거를 소급해서 만들지 않습니다.

`rwd today`는 캐시를 표시하기 전에 현재 수집한 같은 입력 소스와 세션의 발언 ID, 발언자, 파일 경로, 줄 번호와 인용을 대조합니다. 하나라도 맞지 않으면 해당 항목의 모든 근거를 미확인으로 표시하고, 정정은 분류도 미확인으로 바꿉니다. 정정의 대화 순서도 다시 검사합니다. 이 검사는 로컬에서 끝나며, 이 불일치만으로 외부 AI를 다시 호출하지 않습니다. 표시용 결과만 조정하고 저장된 캐시 파일은 유지합니다.

기존 날짜를 새 형식으로 분석하려면 기존 옵션인 `rwd today --date YYYY-MM-DD --no-cache`를 사용할 수 있습니다. 이 명령은 설정된 외부 AI를 호출합니다.

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

Codex messages send only the text itself: the typed `message` or `input_text` for user turns and the `output_text` blocks for answers. Message ids, `phase`/`status` values, and image or file references are not sent. The session's `[META]` line keeps only the working directory, git branch and repository URL, session id, model provider, and source; Codex's built-in instructions and account ids are not sent. Days analyzed before this change keep their cached result; run `rwd today --no-cache --date <date>` to redo one.

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
