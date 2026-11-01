# Release Notes Guide — rwd

## Goal

`rwd` release notes should feel polished, calm, and useful.
We want the tone of a product changelog, but scaled to a small Rust CLI project.

## Preferred Shape

Each GitHub Release should have two layers.

1. Structured summary
   - One-sentence overview
   - `New Features` section when applicable
   - `Highlights` section
   - Small category sections such as `Bug Fixes`
   - Upgrade/install reminder
2. Auto-generated details
   - Full PR list from GitHub release notes
   - Contributors and compare links handled by GitHub

This keeps the top of the release readable while still preserving a full audit trail below.

## Writing Rules

- Start with what changed for the user, not how it was implemented.
- Use counts when they clarify scope, not to exaggerate impact.
- Keep the custom summary short enough to scan in under a minute.
- Let GitHub's generated section carry the exhaustive list.
- Exclude release-only chores such as version bump PRs.
- If setup, config, or behavior changed, mention it in `Highlights`.

## Category Mapping

| PR label | Release section |
| --- | --- |
| `enhancement` | `New Features` |
| `bug` | `Bug Fixes` |
| `performance` | `Performance` |
| `documentation` | `Documentation` |
| `chore`, `dependencies` | `Maintenance` |
| `skip-release-notes` | Excluded from release notes |

## Example Tone

```md
## Release Notes

This release focuses on session discovery reliability and cleaner daily summaries.

### New Features
- support multi-root Claude/Codex log discovery (#85)

### Highlights
- Compared against `v0.13.1` to keep the summary scoped to this release.
- Install with `cargo install --git https://github.com/gigagookbob/rewind.git` or update with `rwd update`.

### Bug Fixes
- parse GitHub release tag_name correctly in installer (#84)
```

## Maintainer Workflow

- `Cargo.toml` version bump still defines the release tag (`vX.Y.Z`).
- `.github/scripts/generate_release_notes.cjs` writes the structured summary.
- `.github/release.yml` controls the GitHub-generated categories.
- PRs opened from `chore/release-vX.Y.Z` branches get `skip-release-notes` automatically.

## Release Highlights in PR Bodies

When a PR changes setup, config, or default behavior, write the user-facing notes in the PR body so they reach `Highlights` without editing the release by hand.

- Section name: a `## Release Highlights` heading (exactly this text, level 2).
- Line format: each top-level line that starts with `- ` under that heading, up to the next heading, is copied as is. Nested bullets, other bullet styles (`*`), plain text, and lines inside code fences are ignored.
- Placement: the lines go at the top of `Highlights`, before the fixed compare, tag, and install lines, in the order the PRs appear in the release.
- PRs labeled `skip-release-notes` contribute no lines. PRs without the section are handled exactly as before.
- Write the lines in English, like the rest of the release notes.

Example PR body:

```md
## Summary
- Adds the Antigravity source adapter.

## Release Highlights
- `rwd today` now reads Antigravity conversations (agy CLI, Antigravity app, Antigravity IDE) by default. Non-interactive `agy -p` runs are skipped.
- To keep Antigravity out of your daily review, add `enabled = false` under `[input.antigravity]` in `~/.config/rwd/config.toml`.

## Test plan
- cargo test --locked
```

Resulting release notes:

```md
### Highlights
- `rwd today` now reads Antigravity conversations (agy CLI, Antigravity app, Antigravity IDE) by default. Non-interactive `agy -p` runs are skipped.
- To keep Antigravity out of your daily review, add `enabled = false` under `[input.antigravity]` in `~/.config/rwd/config.toml`.
- Compared against `v0.13.17` to keep the summary scoped to this release.
- Tagged as `v0.14.0` from commit `abc1234`.
- Install with `cargo install --git https://github.com/gigagookbob/rewind.git` or update with `rwd update`.
```

The script's tests use a fake GitHub client and run without network access. CI does not run them, so run them from the repository root after changing the script:

```bash
node --test .github/scripts/*.test.cjs
```

## When to Edit Manually

The automated summary is the default, and `## Release Highlights` in PR bodies covers most behavior and config changes.
After publishing, it is still worth editing the release body by hand when:

- a release contains behavior changes that users must react to,
- a release introduces a new command or config key,
- a release fixes a subtle data-loss or correctness bug,
- or the automatic bullets miss the real story of the release.

## 0.14.0 릴리스 전 보안 점검 (2026-10-31)

`cargo-audit`가 설치돼 있지 않아 OSV API로 `Cargo.lock`의 crates.io 패키지 276개를 확인했습니다. 버전이 공지와 일치한 패키지는 12개였습니다. 이 결과는 실제 호출 여부를 검사한 결과와 구분합니다.

HTTPS 통신에 포함되는 아래 패키지는 기존 의존성 범위 안에서 갱신했습니다.

- `rustls`: 0.23.37에서 [0.23.46](https://github.com/rustls/rustls/releases/tag/v/0.23.46)으로 갱신했습니다. TLS 처리와 세션 비밀값 노출 관련 수정이 포함됩니다.
- `rustls-webpki`: 0.103.9에서 [0.103.16](https://github.com/rustls/webpki/releases/tag/v/0.103.16)으로 갱신했습니다. 신뢰가 철회된 중간 인증서의 검증 경로를 거부합니다.
- `webpki-roots`: 1.0.6에서 [1.0.11](https://github.com/rustls/webpki-roots/releases/tag/v/1.0.11)로 갱신했습니다. 신뢰가 철회된 루트 인증서를 제거합니다.

갱신 후 다시 검사했을 때 공지와 일치하는 패키지는 9개로 줄었습니다. 다음 공지는 이번 릴리스에서 별도로 처리하지 않았습니다. 재검토 기준일은 2026-11-07입니다.

- `quick-xml`, `quinn-proto`, `rand` 0.9.2: `cargo tree --locked --target <배포 대상> -i <패키지>`로 확인했을 때 macOS ARM64와 Linux x86_64의 활성 의존성에 포함되지 않았습니다. 잠금 파일에는 선택 기능의 의존성으로 남아 있습니다.
- `rpassword` 7.4.0: 입력 중단 시 일부 비밀번호가 보일 수 있다는 낮은 심각도 공지가 있습니다([GHSA-2p6r-x3vv-xqm2](https://github.com/advisories/GHSA-2p6r-x3vv-xqm2)). 설정 입력 경로의 후속 수정으로 남깁니다.
- `tokio` 1.50.0: `AsyncFd` 생성자의 I/O 안전성 공지가 있습니다([RUSTSEC-2026-0386](https://rustsec.org/advisories/RUSTSEC-2026-0386.html)). rwd 소스에서 해당 생성자를 직접 사용하지 않지만, 간접 호출 경로의 안전성은 확인하지 못했습니다.
- `event-listener` 5.4.1: Linux 알림 의존성에 포함됩니다. `StackSlot`에서 스레드로 이동할 수 없는 태그가 다른 스레드로 전달될 수 있다는 공지입니다([RUSTSEC-2026-0221](https://rustsec.org/advisories/RUSTSEC-2026-0221.html)). rwd가 태그를 직접 만들지는 않지만, 간접 호출 경로의 안전성은 확인하지 못했습니다.
- `zbus_names`, `zvariant`, `zvariant_derive`: Linux 알림 의존성에 대한 유지보수 중단 공지입니다. 이번 검사에서 이 세 공지에 연결된 구체적인 취약점은 확인하지 못했습니다.


## 남은 의존성 공지 후속 점검 (2026-10-31)

앞 절에서 남긴 패키지 9개를 다시 확인해 6개를 갱신하거나 제거했습니다. 마지막 잠금 파일의 crates.io 패키지 277개를 OSV API로 검사했을 때 남은 결과는 유지보수 중단 안내 3개였습니다. 유지보수 중단 안내를 제외하면 일치하는 취약점 공지는 없었습니다. 공지 제외 설정은 추가하지 않았습니다.

| 패키지 | 처리 결과 | 근거 |
| --- | --- | --- |
| `rpassword` | 7.4.0에서 7.5.4로 갱신했습니다. | [7.5.0의 입력 중단 시 비밀번호 노출 수정](https://github.com/conradkleinespel/rpassword/releases/tag/v7.5.0)과 이후 호환 수정이 포함됩니다. |
| `tokio` | 1.50.0에서 1.51.6으로 갱신했습니다. | [1.51.6의 입출력 안전성 수정](https://github.com/tokio-rs/tokio/releases/tag/tokio-1.51.6)을 적용한 가장 가까운 수정 계열을 선택했습니다. |
| `event-listener` | 5.4.1에서 5.4.2로 갱신했습니다. | [StackSlot의 Send와 Sync 제한 수정](https://github.com/smol-rs/event-listener/pull/163)이 포함됩니다. Linux 알림 경로에서 컴파일되는 것도 확인했습니다. |
| `quinn-proto` | 0.11.14에서 0.11.19로 갱신했습니다. | [0.11.19의 보안 수정](https://github.com/quinn-rs/quinn/releases/tag/quinn-proto-0.11.19)을 적용했습니다. HTTP/3 기능은 계속 꺼져 있습니다. |
| `rand` | 0.9.2에서 0.10.3으로 교체됐습니다. | 갱신한 `quinn-proto`가 요구하는 0.10 계열로 함께 해석됐습니다. rwd에서 직접 사용하는 의존성은 아닙니다. |
| `quick-xml` | 잠금 파일에서 제거됐습니다. | Windows 전용 부모 패키지 `tauri-winrt-notification`을 0.7.2에서 0.7.3으로 갱신했습니다. [상위 패키지의 수정](https://github.com/tauri-apps/winrt-notification/pull/53)이 XML 라이브러리 의존성을 제거합니다. |
| `zbus_names` | 4.3.1을 유지했습니다. | [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371.html)은 독립 패키지 유지보수 중단 안내입니다. |
| `zvariant` | 5.10.0을 유지했습니다. | [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370.html)은 독립 패키지 유지보수 중단 안내입니다. |
| `zvariant_derive` | 5.10.0을 유지했습니다. | [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372.html)은 독립 패키지 유지보수 중단 안내입니다. |

유지보수가 중단된 세 패키지는 `zbus` 6에 통합됐습니다. 하지만 확인 시점의 [`notify-rust` 최신 안정 버전 4.18.2](https://github.com/hoodie/notify-rust/blob/v4.18.2/Cargo.toml)도 `zbus` 5를 요구합니다. 따라서 이 세 패키지만 6 계열로 올리는 호환 갱신은 할 수 없습니다. 독립 패키지의 마지막 버전으로 올려도 안내는 없어지지 않습니다. [상위 프로젝트의 통합 기록](https://github.com/z-galaxy/zbus/issues/2043)도 이를 확인합니다.

이번에는 별도 포크나 알림 백엔드 전환을 추가하지 않고, 상위 알림 패키지의 안정 버전 대응을 재검토할 항목으로 남겼습니다. 재검토 기준일은 2026-11-07입니다. 구체적인 취약점 수정과 유지보수 중단 안내를 같은 문제로 취급하지 않았습니다.

직접 의존성 범위와 rwd 소스는 바꾸지 않았습니다. 잠금 파일에는 `tokio`가 요구하는 `mio` 1.2.4, `tokio-macros` 2.7.2와 `syn` 3.0.9, `quinn-proto`가 요구하는 난수 라이브러리 변경도 포함됩니다. 각 부모 패키지의 요구 범위 안에서 Cargo가 해석한 결과입니다.

### 검증

- macOS ARM64: 마지막 의존성 변경 뒤 `cargo build --locked`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked --quiet`, `cargo fmt --check`를 통과했습니다. 테스트 319개가 통과했습니다.
- Linux x86_64: Rust 1.94 컨테이너에서 마지막 잠금 파일로 빌드, Clippy와 테스트 319개를 통과했습니다. 소스는 읽기 전용으로 연결했고, 공개 패키지 캐시와 별도의 빌드 폴더를 사용했습니다.
- Linux 컨테이너의 `rwd version` 실행이 정상 종료했습니다.
- macOS의 `RWD_DISABLE_UPDATE_CHECK=1 cli/target/debug/rwd today --dry-run --date 2026-10-31` 실행에서 세션 8개가 수집됐습니다. 이 검사는 LLM을 호출하지 않았습니다.
- OSV 검사 결과는 유지보수 중단 안내 3개입니다. `cargo-audit`는 설치되지 않아 실행하지 않았습니다.
- 실제 비밀번호 입력을 중단하는 실험과 OS 알림 표시는 실행하지 않았습니다. Windows 바이너리 실행도 검증하지 않았으며, 기존의 네이티브 Windows 미지원 조건은 그대로입니다.


### 0.14.1 배포 전 재검증

2026-10-31에 공식 `cargo-audit` 0.22.2의 macOS ARM64 배포 파일을 임시 폴더에 받아 공개 SHA256과 일치하는지 확인했습니다. 새 버전의 잠금 파일을 RustSec 데이터베이스로 검사한 결과 취약점은 0개, 유지보수 중단 안내는 앞서 기록한 3개였습니다. 제외 설정 없이 검사가 정상 종료했습니다. 앞 절의 `cargo-audit` 미실행 기록 이후 추가로 확인한 결과입니다.

`rwd` 버전을 0.14.1로 올린 뒤 macOS에서 빌드, Clippy, 테스트 319개와 포맷 검사를 통과했습니다. 릴리스 노트 테스트 4개도 통과했습니다.

## v0.15.0 회고와 기록 보존 개선 (2026-11-02)

AI 코딩 세션에서 결정과 학습의 근거를 확인하고, 기존 메모를 보존하면서 프로젝트별 회고와 질문 및 후속 작업 상태를 읽을 수 있도록 개선했습니다.

### 새 기능과 수정

- 결정, 학습과 정정에 실제 발언과 원본 파일의 줄 번호를 연결합니다. 사실 오류 정정과 선호 변경을 구분합니다.
- 긴 세션에서는 처음과 마지막 두 조각을 분석하고, 생략한 중간 범위를 표시합니다.
- 과거 날짜 폴더에서 재개한 Codex 세션도 수집합니다. 같은 세션의 이전 발언은 최근 12개, 최대 4,000자까지 참고로 구분합니다.
- 분석과 진행 요약을 별도 자동 생성 영역으로 갱신해, 재실행이나 캐시 재저장 때 수동 메모와 서로의 결과를 보존합니다.
- 직접 분석, 중간 요약, 공유 요약과 재시도에 같은 마스킹 정책을 적용합니다. 일부 API 키와 개인키 본문이 남던 규칙도 보완했습니다.
- 여러 AI 도구의 세션을 작업 폴더 기준으로 묶고, 근거가 있는 작업 제목과 질문 및 후속 작업 상태를 표시합니다. AI의 답변과 사용자의 해결 확인, AI의 제안과 사용자의 요청을 구분합니다.

### 업데이트할 때 확인할 사항

- 표시 없는 기존 일일 노트는 보존한 채 새 자동 영역을 추가하므로, 첫 실행에는 예전 내용과 새 회고가 함께 보일 수 있습니다.
- 자동 영역 안을 사람이 수정했거나 경계가 손상되면 원본을 보존하고 저장을 중단합니다. 수동 메모는 자동 영역 밖에 작성합니다. 복구 방법은 [사용 설명서](https://github.com/gigagookbob/rewind/blob/v0.15.0/cli/README.md)를 참고합니다.
- 이전 버전으로 되돌릴 때는 출력 폴더를 분리합니다. 이전 버전에는 이 자동 영역 보존 기능이 없으므로 같은 노트를 다시 생성하지 않습니다.
- 입력 정책이 바뀌어 이전 분석 캐시는 다음 실행에서 다시 분석될 수 있습니다. 기존 사용자 캐시와 노트를 일괄 삭제하거나 변환하지 않습니다.
- 마스킹은 기본 활성화이며 `redactor.enabled = false`이면 꺼집니다. 내장 규칙 밖의 비밀값 탐지를 보장하지 않으며, 보수적인 검사 때문에 일반 문장도 가려질 수 있습니다.
- 질문과 후속 작업은 대화에서 확인한 상태입니다. 완료 보고는 실제 실행 성공을 독립적으로 검증했다는 뜻이 아닙니다.

### 검증과 한계

- macOS에서 v0.15.0 빌드, Clippy, 전체 421개 테스트와 포맷 검사를 확인했습니다.
- 공식 `cargo-audit` 0.22.2와 갱신한 RustSec 데이터베이스에서 취약점은 0개였습니다. 기존 Linux 알림 의존성의 유지보수 중단 안내 3개는 남아 있습니다.
- 실제 전송과 같은 HTTP 요청 및 Codex 입력을 가짜 전송으로 검사했습니다. 실제 외부 AI의 인증과 의미 추출 품질은 검증하지 않았습니다.
- 긴 세션의 중간 생략과 과거 참고 한도, 외부 편집기와 마지막 순간에 동시에 저장하는 경우의 한계는 남습니다.

### 업데이트

기존 설치에서는 `rwd update`를 실행합니다. 배포 파일은 macOS Apple Silicon과 Linux x86_64를 제공합니다.

[전체 변경 비교](https://github.com/gigagookbob/rewind/compare/v0.14.1...v0.15.0)
