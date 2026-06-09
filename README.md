# rewind

`rewind` 모노레포 — AI 코딩 세션을 분석해 일일 개발 인사이트를 뽑아내는 CLI(`rwd`)와 그 랜딩 페이지를 한 저장소에서 관리합니다.

## 구조

```
rewind/
├── cli/        # rwd — Rust CLI 도구 (AI 세션 로그 분석 → Obsidian Daily Notes)
└── website/    # 랜딩 페이지 (순수 HTML/CSS/JS, Vercel 배포)
```

각 하위 프로젝트는 자체 문서(`AGENTS.md`, `README.md`/`DESIGN.md`)와 `.gitignore`, 도구 설정을 그대로 유지합니다.

## cli/

AI 에이전트 세션을 일일 저널로 변환하는 Rust CLI입니다. 자세한 사용법은 [cli/README.md](cli/README.md) 참고.

```bash
cd cli
cargo build            # 빌드
cargo run -- today     # 오늘 세션 분석
cargo clippy           # 린트
cargo test             # 테스트
```

## website/

`rwd`의 정적 랜딩 페이지입니다. 빌드 스텝 없이 파일을 직접 수정합니다. 자세한 내용은 [website/AGENTS.md](website/AGENTS.md) 참고.

```bash
cd website
vercel dev             # 로컬 개발 서버
vercel --prod          # 프로덕션 배포
```

## 비고

- 이 모노레포는 기존 `rwd`, `rwd-website` 저장소의 추적 파일 스냅샷으로 새로 시작했습니다 (커밋 히스토리는 포함하지 않음).
- 원격 저장소는 아직 연결하지 않았습니다 (로컬 전용).
