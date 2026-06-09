# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project Overview

`rwd` CLI 도구의 정적 랜딩 페이지. 순수 HTML/CSS/JS로 구성되며 빌드 도구 없이 Vercel에 배포된다.

## Development

빌드 스텝, 패키지 매니저, 린터 없음. 파일을 직접 수정하면 된다.

```bash
# 로컬 개발 서버 (Vercel CLI)
vercel dev

# 배포
vercel          # preview
vercel --prod   # production
```

## Architecture

단일 페이지 정적 사이트. 핵심 엔트리 파일:

- `index.html` — 전체 마크업 (nav, hero, terminal demo, comparison, features, install, output example, FAQ, footer)
- `css/00-tokens.css` — 디자인 토큰(`:root`)
- `css/01-base.css` — reset/base
- `css/02-layout.css` — container/section/divider
- `css/03-components.css` — nav/button/terminal/comparison/install/faq/footer
- `css/04-showcase.css` — showcase/mockup 전용
- `css/05-utilities.css` — animation/text/spacing utilities
- `css/06-responsive.css` — 640px 브레이크포인트 반응형
- `js/main.js` — analytics 로더, 클립보드 복사, FAQ 아코디언, IntersectionObserver 스크롤 fade-in

## Design System

CSS custom properties가 `css/00-tokens.css`의 `:root`에 정의되어 있다. 다크 테마(zinc 계열) 단일 모드.

- `--bg-primary: #09090b`, `--bg-secondary: #18181b` — 배경
- `--text-primary/secondary/muted/dim` — 텍스트 계층
- `--border: #27272a` — 테두리
- `--max-width: 960px` — 컨테이너 최대 폭
- 폰트: system sans-serif + monospace (SF Mono, Fira Code 등)

## DESIGN.md Workflow

이 저장소는 `awesome-design-md` 스타일 워크플로우를 도입했다.

- 루트의 `DESIGN.md`를 디자인 규칙의 단일 소스로 사용한다.
- UI/스타일 변경 전에는 `DESIGN.md` + `css/00-tokens.css`를 먼저 확인한다.
- 시각 규칙(토큰, 타이포 스케일, 컴포넌트 상태, 반응형 동작)을 바꾸면 `DESIGN.md`를 같은 변경에서 함께 업데이트한다.

## Deployment

Vercel 정적 배포. `vercel.json`에 `css/`, `js/`, `assets/` 경로에 대해 1년 immutable 캐시 헤더가 설정되어 있다.

## Conventions

- 커밋 메시지: conventional commits (`feat:`, `fix:`, `style:`, `refactor:`, `docs:`)
- i18n 시스템은 제거됨 — 현재 영어 전용
