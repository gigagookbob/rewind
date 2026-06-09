# Feature Showcase Sections — Design Spec

## Overview

Superset.sh 스타일의 피처 쇼케이스 섹션 6개를 추가한다. 각 섹션은 rwd의 핵심 동작을 텍스트 설명 + macOS 윈도우 목업으로 보여준다.

## Decisions

| 항목 | 결정 | 이유 |
|------|------|------|
| 섹션 수 | 6개 | rwd의 주요 동작 6가지를 각각 풀 너비로 보여줌 |
| 레이아웃 | 2컬럼 교차 (텍스트/목업 좌우 번갈아) | Superset.sh 레퍼런스와 동일한 패턴 |
| 목업 구현 | 순수 HTML/CSS (이미지 없음) | 기존 스택(빌드 도구 없음)과 일관성, 텍스트 수정 용이 |
| 기존 섹션 | "What rwd extracts" + "From sessions to journal" 삭제 | 새 쇼케이스가 동일 내용을 더 효과적으로 전달 |
| 위치 | Comparison 섹션 아래 ~ Install 섹션 위 | 제품 이해 흐름: 차별점 → 동작 상세 → 설치 |
| 애니메이션 | 기존 `.fade-in` IntersectionObserver 재활용 | 추가 JS 불필요 |

## Page Flow

```
Nav → Hero → Terminal Demo → Comparison →
  Showcase 1: Session Discovery
  Showcase 2: Smart Extraction
  Showcase 3: Privacy
  Showcase 4: Daily Notes
  Showcase 5: Work Summary
  Showcase 6: Slack Message
→ Install → FAQ → Footer CTA → Footer
```

## Layout System

### Desktop (>= 640px)

- 2컬럼 grid: 텍스트 45% / 목업 55%
- 홀수 섹션 (1, 3, 5): 텍스트 왼쪽, 목업 오른쪽
- 짝수 섹션 (2, 4, 6): 목업 왼쪽, 텍스트 오른쪽
- 수직 중앙 정렬 (align-items: center)
- 섹션 간 divider (`--border` 색상)

### Spacing

- 쇼케이스 섹션 패딩: `80px 24px` (기존 `--section-padding` 동일)
- 텍스트 컬럼 내부 gap: label→title `12px`, title→desc `16px`
- 목업 body 패딩: `20px`
- 쇼케이스 블록 경계에만 divider: Comparison↔첫 쇼케이스, 마지막 쇼케이스↔Install
- 쇼케이스 간에는 divider 없음 (패딩으로 충분한 분리)

### Mobile (< 640px)

- 단일 컬럼, 세로 스택
- 순서: 목업 먼저 → 텍스트 아래 (모든 섹션 동일)
- 목업 max-width: 100%

### 텍스트 컬럼 구조

```
[LABEL]          — 11px uppercase, letter-spacing 2px, --text-muted
[TITLE]          — 28px bold, --text-primary, letter-spacing -0.5px
[DESCRIPTION]    — 15px, --text-secondary, line-height 1.6, max 2줄
```

### 목업 윈도우 구조

```
┌─ ● ● ● ── [Title] ──────────────┐
│                                   │
│  [Content: terminal / note / code]│
│                                   │
└───────────────────────────────────┘
```

- macOS 윈도우 크롬: 빨/노/초 dots + 타이틀
- 배경: `--bg-secondary`
- 테두리: `--border`
- border-radius: 12px
- 기존 `.terminal` 스타일 재활용 가능
- 목업 타이틀: dots 오른쪽에 left-aligned (기존 `.terminal-title` 패턴)

### Obsidian 목업 (섹션 4) 내부 레이아웃

- 사이드바: 35% / 노트 프리뷰: 65%
- 사이드바와 프리뷰 사이 `1px solid var(--border)` 구분선

### 인라인 색상 적용 방식

목업 내부의 색상 강조는 `<span>` + CSS 클래스로 적용:

```css
.mc-dim     { color: var(--text-dim); }
.mc-primary { color: var(--text-primary); }
.mc-green   { color: var(--accent-green); }
.mc-muted   { color: var(--text-muted); }
.mc-bold    { color: var(--text-primary); font-weight: 600; }
```

`mc-` 접두사 = mockup color.

## Section Details

### 1. Session Discovery

| 항목 | 값 |
|------|-----|
| 레이블 | `SESSION DISCOVERY` |
| 제목 | One command. Every session. |
| 설명 | Run `rwd today` and it scans all your AI coding sessions automatically. Claude Code, Codex — no manual export needed. |
| 배치 | 텍스트 LEFT, 목업 RIGHT |
| 목업 타입 | 터미널 |
| 목업 타이틀 | Terminal |

목업 내용:
```
$ rwd today
⠋ Discovering sessions...
  Found 3 Claude Code sessions
  Found 1 Codex session
⠋ Planning execution...
  Strategy: single-shot (4 sessions, ~12k tokens)
⠋ Analyzing with Claude API...
```

색상:
- `$` 프롬프트: `--text-dim`
- `rwd today`: `--text-primary`
- `⠋` 스피너: `--accent-green`
- 들여쓴 결과: `--text-secondary`
- `Found N`: `--text-primary` (숫자 강조)

### 2. Smart Extraction

| 항목 | 값 |
|------|-----|
| 레이블 | `SMART EXTRACTION` |
| 제목 | Finds what actually matters |
| 설명 | Not a log dump. rwd identifies decisions you made, questions you had, mistakes the AI made, and things you learned. |
| 배치 | 목업 LEFT, 텍스트 RIGHT |
| 목업 타입 | 마크다운 노트 |
| 목업 타이틀 | 2025-03-24.md |

목업 내용:
```markdown
## Key Decisions
- Axum over Actix-web: Simpler middleware API

## Curiosities
- How does tower middleware chain work?

## Corrections
- Model said: Use .layer() for auth
  Fix: Must wrap in ServiceBuilder first

## TIL
- Tower ServiceBuilder ordering matters
```

색상:
- `##` 헤딩: `--text-primary`, 15px bold
- 본문: `--text-secondary`
- `Model said:` / `Fix:` 라벨: `--text-muted` / `--accent-green`

### 3. Privacy

| 항목 | 값 |
|------|-----|
| 레이블 | `PRIVACY FIRST` |
| 제목 | Sensitive data never leaves |
| 설명 | API keys, AWS credentials, GitHub tokens, private IPs — automatically masked before LLM analysis. Eight patterns catch secrets you might miss. |
| 배치 | 텍스트 LEFT, 목업 RIGHT |
| 목업 타입 | 코드 뷰 (before/after) |
| 목업 타이틀 | Redactor |

목업 내용:
```
export OPENAI_KEY=sk-abc123def456ghi789...
Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWI...
Server: 192.168.1.42:3000

        ↓

export OPENAI_KEY=[REDACTED:API_KEY]
[REDACTED:BEARER_TOKEN]
Server: [REDACTED:PRIVATE_IP]:3000

✓ Redacted: API_KEY: 1, BEARER_TOKEN: 1, PRIVATE_IP: 1
```

색상:
- Before 텍스트: `--text-dim`
- `↓` 화살표: `--text-dim`
- `[REDACTED:*]`: `--accent-green`
- `✓` 결과 줄: `--accent-green`

### 4. Daily Notes

| 항목 | 값 |
|------|-----|
| 레이블 | `DAILY NOTES` |
| 제목 | Lands right in your vault |
| 설명 | Structured markdown saved to your Obsidian vault or any directory. Each session becomes a documented chapter of your day. |
| 배치 | 목업 LEFT, 텍스트 RIGHT |
| 목업 타입 | Obsidian 스타일 (사이드바 + 노트) |
| 목업 타이틀 | Obsidian |

목업 내용:
```
[사이드바]                    [노트 프리뷰]
📁 Daily                     # 2025-03-24 Dev Session Review
  📄 2025-03-24.md  ← active
  📄 2025-03-23.md            ## Claude Code
  📄 2025-03-22.md            ### Work Summary
                              Implemented order validation...
                              ### Key Decisions
                              - Axum over Actix-web: ...
```

색상:
- 사이드바 배경: `--bg-primary` (대비)
- 활성 파일: `--text-primary` + 좌측 accent 바
- 비활성 파일: `--text-dim`
- 노트 프리뷰: 섹션 2와 동일한 마크다운 색상

### 5. Work Summary

| 항목 | 값 |
|------|-----|
| 레이블 | `WORK SUMMARY` |
| 제목 | Ready for standup |
| 설명 | `rwd summary` generates a concise work report grouped by project. Non-technical language, copied to clipboard — paste straight into your status update. |
| 배치 | 텍스트 LEFT, 목업 RIGHT |
| 목업 타입 | 터미널 |
| 목업 타이틀 | Terminal |

목업 내용:
```
$ rwd summary

### E-commerce API
- Order validation logic was completed
- Payment webhook handling was resolved

### Internal Dashboard
- User analytics chart was implemented

✓ Copied to clipboard
```

색상:
- `$` 프롬프트: `--text-dim`
- `rwd summary`: `--text-primary`
- `###` 프로젝트명: `--text-primary`
- 불릿 항목: `--text-secondary`
- `✓ Copied`: `--accent-green`

### 6. Slack Message

| 항목 | 값 |
|------|-----|
| 레이블 | `SLACK MESSAGE` |
| 제목 | Share without the jargon |
| 설명 | `rwd slack` turns your technical day into a team-friendly update. No file paths, no branch names — just clear outcomes ready to post. |
| 배치 | 목업 LEFT, 텍스트 RIGHT |
| 목업 타입 | 터미널 |
| 목업 타이틀 | Terminal |

목업 내용:
```
$ rwd slack

[Today's Work Update]
- Order validation feature was completed
- Payment processing issue was identified and fixed
- User analytics dashboard was implemented

✓ Copied to clipboard
```

색상:
- `$` 프롬프트: `--text-dim`
- `rwd slack`: `--text-primary`
- `[Today's Work Update]`: `--text-primary`, bold
- 불릿 항목: `--text-secondary`
- `✓ Copied`: `--accent-green`

## CSS Architecture

### 새로 추가할 클래스

```css
/* 쇼케이스 섹션 컨테이너 */
.showcase {}
.showcase-inner {}           /* 2컬럼 grid */
.showcase-inner--reversed {} /* 짝수 섹션: 목업 먼저 */

/* 텍스트 컬럼 */
.showcase-label {}
.showcase-title {}
.showcase-desc {}

/* 목업 윈도우 (기존 .terminal 확장) */
.mockup {}
.mockup-header {}            /* dots + title */
.mockup-body {}

/* 목업 내부 변형 */
.mockup-terminal {}          /* 터미널 출력용 */
.mockup-note {}              /* 마크다운 노트용 */
.mockup-code {}              /* 코드 뷰용 */
.mockup-obsidian {}          /* 사이드바 + 노트 분할용 */
```

### 반응형

```css
@media (max-width: 640px) {
  .showcase-inner {
    grid-template-columns: 1fr;
  }
  .showcase-inner .mockup {
    order: -1; /* 모든 섹션에서 목업이 항상 위에 */
  }
}
```

## HTML Removal Map

삭제할 기존 블록 (현재 `index.html` 기준):

| 삭제 대상 | 라인 범위 | 설명 |
|-----------|----------|------|
| Features divider | line 185 | `<div class="divider"></div>` |
| Features section | lines 187–229 | "What rwd extracts" 4-card grid |
| Features→Install divider | line 231 | `<div class="divider"></div>` |
| Output divider | line 297 | `<div class="divider"></div>` |
| Output Example section | lines 299–337 | "From sessions to journal" before→after |
| Output→FAQ divider | line 339 | `<div class="divider"></div>` |

## CSS Cleanup

삭제 가능한 기존 CSS 클래스:

| 클래스 | 이유 |
|--------|------|
| `.features-grid` | 쇼케이스가 대체 |
| `.feature-card` | 쇼케이스가 대체 |
| `.feature-name` | 쇼케이스가 대체 |
| `.feature-desc` | 쇼케이스가 대체 |
| `.output-split` | 쇼케이스가 대체 |
| `.output-panel` | 쇼케이스가 대체 |
| `.output-before` | 쇼케이스가 대체 |
| `.output-after` | 쇼케이스가 대체 |
| `.output-label` | 쇼케이스가 대체 |
| `.output-code` | 쇼케이스가 대체 |
| `.output-arrow` | 쇼케이스가 대체 |

유지할 기존 CSS 클래스:

| 클래스 | 이유 |
|--------|------|
| `.md-heading` | 섹션 2, 4 목업에서 재활용 |
| `.md-text` | 섹션 2, 4 목업에서 재활용 |

## Files Modified

| 파일 | 변경 |
|------|------|
| `index.html` | 위 HTML Removal Map 블록 삭제, 6개 쇼케이스 섹션 추가 |
| `css/style.css` | 위 CSS Cleanup 클래스 삭제, `.showcase-*`, `.mockup-*`, `.mc-*` 스타일 추가, 반응형 규칙 추가 |
| `js/main.js` | 변경 없음 (기존 `.fade-in` 로직이 새 섹션도 처리) |

## Out of Scope

- 목업 내 애니메이션 (타이핑 효과 등) — 정적 HTML만
- 목업 클릭 인터랙션
- Superset.sh의 배경 aurora/gradient 효과
