# rwd Landing Page Design System

## 1. Visual Theme & Atmosphere

rwd uses a terminal-native dark interface designed for developer tooling. The visual style is minimal and content-first: deep zinc backgrounds, subtle 1px separators, and compact monochrome typography with small semantic accents.

The site should feel quiet and trustworthy rather than decorative. Interface depth comes from layered surfaces (`--bg-primary` and `--bg-secondary`) and border contrast (`--border`), not from heavy shadows or gradients.

Key characteristics:
- Single dark theme only (no light mode)
- Sticky translucent nav (`rgba(9, 9, 11, 0.85)`) with blur
- High contrast title text on near-black canvas
- Monospace usage for code/command context only
- Green accent reserved for success and positive states
- Soft entrance animation with `.fade-in` and IntersectionObserver

## 2. Color Palette & Roles

### Core Tokens
- **Background Primary** (`#09090b`): page background, sidebar surfaces
- **Background Secondary** (`#18181b`): cards, terminal, mockup containers
- **Border** (`#27272a`): separators, card outlines, controls
- **Text Primary** (`#fafafa`): headings, strong labels, active items
- **Text Secondary** (`#a1a1aa`): default body copy
- **Text Muted** (`#a1a1aa`): captions, helper text, labels (WCAG AA 4.5:1 on `--bg-primary`)
- **Text Dim** (`#8a8a94`): inactive states, secondary chrome text (WCAG AA 4.5:1 on `--bg-primary` and `--bg-secondary`)

### Semantic Accent Tokens
- **Accent Green** (`#22c55e`): success states, copied state, active/positive indicators
- **Accent Red** (`#ef4444`): terminal dot (close)
- **Accent Yellow** (`#eab308`): terminal dot (minimize)
- **CTA Background** (`#fafafa`): primary button background
- **CTA Text** (`#09090b`): primary button text

### Role Guidelines
- Keep all layout/chrome colors on the zinc scale.
- Use green as the only broad semantic accent in content states.
- Use red/yellow only for macOS-style window dots.
- Avoid adding decorative neon/gradient palettes.

## 3. Typography Rules

### Font Family
- **Sans (default)**: `-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif`
- **Mono**: `'SF Mono', 'Fira Code', 'Cascadia Code', 'Consolas', monospace`

### Hierarchy

| Role | Size | Weight | Line Height | Letter Spacing | Color |
|---|---:|---:|---:|---:|---|
| Hero Title (`.hero-title`) | 72px | 700 | 1.1 | -3px | `--text-primary` |
| Hero Title Mobile | 42px | 700 | 1.1 | -2px | `--text-primary` |
| Footer CTA Title (`.footer-cta-title`) | 32px | 600 | default | -0.5px | `--text-primary` |
| Install Title (`.install-title`) | 32px | 600 | default | -0.5px | `--text-primary` |
| Showcase Title (`.showcase-title`) | 28px | 700 | 1.2 | -0.5px | `--text-primary` |
| Section Title (`.section-title`) | 24px | 600 | default | -0.5px | `--text-primary` |
| Nav Logo (`.nav-logo`) | 22px | 700 | default | -0.5px | `--text-primary` |
| Body Large (`.hero-subtitle`) | 16px | 400 | 1.6 | normal | `--text-muted` |
| Body (`body`, `.showcase-desc`) | 15-16px | 400 | 1.6 | normal | `--text-secondary` |
| UI Text (`.btn`, `.faq-question`) | 13-14px | 600 | ~1.35-1.6 | slight tracking on FAQ | token-based |
| Small Labels (`.comparison-label`, `.install-platform`) | 12px | 400 | default | 1px uppercase (where used) | `--text-muted` |
| Mono Command (`.install-cmd`, `.mockup-body`) | 12-13px | 400 | 1.6-1.8 | normal | `--text-secondary` |

### Principles
- Preserve tight tracking on key titles (`-3px`, `-2px`, `-0.5px`).
- Use sans for product copy; mono only for commands/terminal/note previews.
- Keep weight distribution simple (400/600/700).

## 4. Component Stylings

### Navigation
- Sticky top navigation with blur backdrop and bottom border.
- Full-width container (no max-width cap) with `10px 0` inner spacing and `320px` horizontal margin.
- Right side contains docs link and GitHub button; button uses bordered ghost style.

### Buttons
- Base `.btn`: inline-flex, 10px x 24px, 8px radius, 14px/600.
- **Primary**: light background (`--cta-bg`) with dark text (`--cta-text`).
- **Secondary**: transparent with 1px border; hover fills `--bg-secondary`.

### Terminal & Mockup Window Pattern
- Shared shape language:
  - Background `--bg-secondary`
  - Border `1px solid --border`
  - Radius 10-12px
  - Header with three colored dots (red/yellow/green) and dim mono title text
- The hero terminal (`.terminal-live`) renders real `rwd today` output as live HTML (banner, summary card, progress lines, one session preview). No raster image; content is selectable text so it scales and reflows with the viewport.
- Inside terminal/mockup bodies, authentic ANSI-inspired color mapping is allowed even outside the zinc scale: cyan `#22d3ee` (banner), bright blue `#60a5fa` (Claude Code / session headers), yellow `#facc15` (Codex / section labels), magenta `#c084fc` (analysis step), plus `--accent-red`/`--accent-green` for the `✗ / ✓` correction pair.

### Comparison Cards
- 2-column grid desktop, single-column mobile.
- Card style: 24px padding, 10px radius, bordered dark surface.
- `comparison-rwd` has slightly brighter border emphasis.

### Install Blocks
- Vertical stack of command rows.
- Each row has platform label + mono command + icon-only copy button.
- Copy states:
  - Default: dim border/text
  - Hover: brighter border/text
  - Copied: green border/text (`.copy-btn.copied`)

### FAQ
- Two-column layout on desktop: left title block and right border-line accordion list.
- Desktop ratio: title column `240-320px`, list column up to `760px`.
- List rows use subtle low-contrast divider lines for a minimal terminal-like feel.
- Clicking one item opens it and closes others.
- Open state rotates plus icon by 45 degrees.

### Showcase Sections
- Four alternating sections: Capture, Rewind, Redact, Share. Each section has a short eyebrow label (`01 · Capture` …), a 30px title, 1–2 fragment desc, and a distinct mockup artifact on the opposite side.
- Desktop: 45/55 text/mockup split (`.showcase-inner`, `grid-template-columns: 45fr 55fr`, 64px gap).
- Alternating layout uses `.showcase-inner--reversed` with the `direction: rtl` flip trick. On mobile the layout stacks and the mockup always sits above the text regardless of direction.
- Section rhythm is `96px 0` on desktop, `64px 0` on mobile.
- Mockup variants (all share `.mockup` chrome):
  - `.mockup-progress` — mono terminal of `rwd today` analysis steps (Block 1, Capture). Real session IDs, magenta for "Analyzing…", green `✓` for done steps.
  - `.mockup-note` — rendered Markdown daily note (Block 2, Rewind). Mock title, `## Session:` heading in bright blue, uppercase `H3` labels, bullet lists, and a red/green Model/Fix correction pair. Fades out at the bottom via a secondary-background linear-gradient to imply there is more content below.
  - `.mockup-diff` — side-by-side Redact view (Block 3). Left column tinted red `rgba(239,68,68,0.04)` with `.diff-secret` strikethrough; right column tinted green `rgba(34,197,94,0.04)` with `.diff-mask` tokens like `[REDACTED:API_KEY]`.
  - `.mockup-chat` — Slack-style post (Block 4, Share). Channel header, rounded gradient avatar, message body with parenthesized category tags and a bottom-bar `Copied to clipboard` confirmation in `--accent-green`.

### Footer
- Top CTA strip with border-top and centered call-to-action.
- Final footer uses compact muted metadata with subtle hover brightening.

## 5. Layout Principles

### Containers & Width
- Global content container max width: none (full-width layout).
- Direct page wrappers (`.nav-inner`, `section > .container`, `section > .showcase-inner`, `footer > .container`) use a shared fluid `--gutter` token: `clamp(16px, calc((100vw - var(--content-width)) / 2), 320px)` with `--content-width: 960px`.
- That formula caps gutters at `320px` on wide screens (≥1600px viewport, matching the original design), shrinks them proportionally between 960–1600px so the inner content holds at ~960px, and floors at `16px` below 960px so content keeps comfortable edge padding all the way down to ~320px.

### Section Rhythm
- Default section padding: `clamp(48px, 6vw, 80px) 0`.
- Hero: `clamp(48px, 6vw, 80px) 0 clamp(24px, 4vw, 40px)`.
- Terminal section: `clamp(64px, 12vw, 160px) 0 clamp(40px, 6vw, 80px)`.
- Showcase: `clamp(56px, 7vw, 96px) 0`.
- Footer CTA: `clamp(56px, 7vw, 80px) 0`.
- Major sections separated with 1px divider line using `--border`.

### Grid Usage
- Showcase: `grid-template-columns: 45fr 55fr`, 64px gap.
- Redact diff: `grid-template-columns: 1fr 1fr` inside `.mockup-diff .mockup-body` (stacks to `1fr` on mobile).
- Summary card (hero): single column of bordered agent rows with a date head strip.

### Spacing Behavior
- Frequent spacing values: 4, 6, 8, 10, 12, 16, 20, 24, 32, 40, 48, 80.
- Keep spacing conservative and consistent; avoid introducing unrelated spacing scales.

## 6. Depth & Elevation

Depth model is intentionally flat and controlled.

| Level | Treatment | Use |
|---|---|---|
| L0 | `--bg-primary` | Base page background |
| L1 | `--bg-secondary` + 1px border | Cards, terminal, mockups, install blocks |
| L2 | Sticky translucent layer + blur | Top navigation only |

Rules:
- No drop shadows in default component system.
- Use border contrast and surface layering for separation.
- Keep radii in a narrow set: 4px, 6px, 8px, 10px, 12px.

## 7. Do's and Don'ts

### Do
- Keep the zinc dark theme and existing token names.
- Preserve terminal-inspired chrome (dot header + bordered surfaces).
- Use `--accent-green` for successful/positive state feedback.
- Reuse existing component classes before adding new abstractions.
- Maintain concise copy density and readable line-height around 1.6.

### Don't
- Do not add light mode, gradients, or colorful backgrounds without explicit product direction.
- Do not replace the system font stack with decorative web fonts.
- Do not use strong shadows/glows as primary separation.
- Do not use red/yellow accents outside terminal dot metaphors.
- Do not add dense animation beyond existing fade-in/reveal patterns.

## 8. Responsive Behavior

Sizing flows from `clamp()` and the fluid `--gutter` token, so the layout adapts continuously between ~320px and 4K. Media queries are reserved for **structural** changes that fluid sizing alone cannot express (collapsing grids, hiding nav chrome).

### Fluid type & spacing
- Hero title: `clamp(36px, 7vw, 72px)`, letter-spacing `clamp(-3px, -0.25vw, -1px)`.
- Showcase title: `clamp(24px, 2.6vw, 30px)`.
- FAQ title: `clamp(24px, 2.6vw, 30px)`.
- Install title / Footer CTA title: `clamp(22px, 2.8vw, 32px)`.
- Showcase column gap: `clamp(32px, 5vw, 64px)`.
- All major section paddings use `clamp()` (see Section Rhythm).
- `--gutter` provides consistent fluid horizontal padding everywhere.

### Breakpoint: `max-width: 768px` (tablet)
- Showcase 45/55 grid collapses to one column. The mockup sits above the text (via `order`) regardless of the desktop `direction: rtl` flip.

### Breakpoint: `max-width: 640px` (mobile)
- Hide GitHub button text label in nav (`.nav-github-btn span`).
- Hero CTA stack becomes vertical; buttons fill width up to 280px.
- Hero terminal: banner ASCII drops to 8px, summary card expands to fill width, meta rows tighten.
- Mockup variants tighten font/padding; note mockup `max-height` drops to 440px so the fade-out stays visible.
- Redact diff stacks raw → masked vertically with a horizontal border between them.
- Chat mockup avatar shrinks 40→32px.
- FAQ two-column layout collapses to one column.
- Footer top stacks vertically.

### Breakpoint: `max-width: 420px` (small phones)
- Install block tightens padding and reduces command font to 12px.
- Footer nav allows wrapping.

### Touch and readability
- Buttons and FAQ rows stay at desktop tap-target sizes.
- Mockup content allows horizontal scroll where needed (the terminal body has `overflow-x: auto`).
- Body has `overflow-x: hidden` to prevent any incidental horizontal scroll from off-screen elements.
- Preserve mono command legibility in install blocks and mockups.

## 9. Agent Prompt Guide

### Quick Style Reference
- Page background: `#09090b`
- Surface background: `#18181b`
- Border: `#27272a`
- Primary text: `#fafafa`
- Body text: `#a1a1aa`
- Muted text: `#a1a1aa`
- Success accent: `#22c55e`
- Primary CTA: background `#fafafa`, text `#09090b`

### Prompt Snippets
- "Build a dark developer landing section for rwd using zinc tones only. Use background #09090b, bordered surface cards on #18181b, and 1px #27272a separators."
- "Create a hero with 72px bold heading, letter-spacing -3px, centered layout, muted 16px subtitle, and two CTAs (solid light primary + outlined secondary)."
- "Design a terminal-style mockup window with a 12px radius, 1px border, macOS red/yellow/green dots, and monospace 13px body text."
- "Add an install command row: platform label (12px muted), monospace command, and icon-only copy button with green copied state."
- "Implement FAQ accordion behavior: single-open-item pattern, smooth max-height transition, and icon rotation on open."

### Iteration Checklist
1. Check whether new style values already exist in `css/00-tokens.css`.
2. Prefer updating existing component classes in `css/03-components.css` and `css/04-showcase.css`.
3. Keep mobile behavior aligned with `css/06-responsive.css` at 640px breakpoint.
4. If visual rules change materially, update this `DESIGN.md` in the same change.
