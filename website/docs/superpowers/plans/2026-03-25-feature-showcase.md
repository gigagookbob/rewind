# Feature Showcase Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace existing feature cards and output example sections with 6 Superset.sh-style showcase sections featuring alternating text/mockup layouts.

**Architecture:** Pure HTML/CSS additions to a static site. Remove old sections (Features grid, Output Example), add 6 new `.showcase` sections with `.mockup` frames. No JS changes needed — existing IntersectionObserver handles fade-in.

**Tech Stack:** HTML, CSS (custom properties), no build tools.

**Spec:** `docs/superpowers/specs/2026-03-25-feature-showcase-design.md`

---

### Task 1: CSS — Add showcase styles

**Files:**
- Modify: `css/style.css`

- [ ] **Step 1: Add showcase + mockup + mc-\* styles**

Add after the Comparison CSS section (after `.comparison-list`), before `/* === Features === */`:

```css
/* === Showcase === */
.showcase {
  padding: var(--section-padding);
}

.showcase-inner {
  display: grid;
  grid-template-columns: 45fr 55fr;
  gap: 40px;
  align-items: center;
  max-width: var(--max-width);
  margin: 0 auto;
}

.showcase-inner--reversed {
  direction: rtl;
}

.showcase-inner--reversed > * {
  direction: ltr;
}

.showcase-text {
  display: flex;
  flex-direction: column;
}

.showcase-label {
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 2px;
  color: var(--text-muted);
  margin-bottom: 12px;
}

.showcase-title {
  font-size: 28px;
  font-weight: 700;
  color: var(--text-primary);
  letter-spacing: -0.5px;
  line-height: 1.2;
  margin-bottom: 16px;
}

.showcase-desc {
  font-size: 15px;
  color: var(--text-secondary);
  line-height: 1.6;
}

.showcase-desc code {
  font-family: var(--font-mono);
  font-size: 13px;
  background: var(--bg-secondary);
  padding: 2px 6px;
  border-radius: 4px;
}

/* === Mockup Window === */
.mockup {
  background: var(--bg-secondary);
  border-radius: 12px;
  border: 1px solid var(--border);
  overflow: hidden;
}

.mockup-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border);
}

.mockup-dots {
  display: flex;
  gap: 6px;
}

.mockup-dots span {
  width: 10px;
  height: 10px;
  border-radius: 50%;
}

.mockup-dots span:nth-child(1) { background: var(--accent-red); }
.mockup-dots span:nth-child(2) { background: var(--accent-yellow); }
.mockup-dots span:nth-child(3) { background: var(--accent-green); }

.mockup-title {
  font-size: 12px;
  color: var(--text-dim);
  margin-left: 8px;
}

.mockup-body {
  padding: 20px;
  font-family: var(--font-mono);
  font-size: 13px;
  line-height: 1.8;
}

/* Mockup variants */
.mockup-terminal .mockup-line {
  white-space: pre;
}

.mockup-note .mockup-body {
  font-family: var(--font-sans);
}

.mockup-note .md-heading {
  font-size: 15px;
}

.mockup-note .md-heading + .md-heading,
.mockup-body .md-heading:not(:first-child) {
  margin-top: 16px;
}

.mockup-code .mockup-body {
  font-size: 12px;
}

.mockup-obsidian .mockup-body {
  display: flex;
  padding: 0;
  font-family: var(--font-sans);
}

.mockup-sidebar {
  width: 35%;
  padding: 16px;
  background: var(--bg-primary);
  border-right: 1px solid var(--border);
  font-size: 13px;
  line-height: 2;
}

.mockup-sidebar-item {
  color: var(--text-dim);
  padding-left: 12px;
}

.mockup-sidebar-item--active {
  color: var(--text-primary);
  border-left: 2px solid var(--accent-green);
  padding-left: 10px;
}

.mockup-preview {
  width: 65%;
  padding: 16px 20px;
  font-size: 13px;
  line-height: 1.6;
}

/* Mockup inline colors */
.mc-dim { color: var(--text-dim); }
.mc-primary { color: var(--text-primary); }
.mc-green { color: var(--accent-green); }
.mc-muted { color: var(--text-muted); }
.mc-bold { color: var(--text-primary); font-weight: 600; }

/* Mockup spacing utilities */
.mockup-sep { margin: 12px 0; text-align: center; }
.mockup-gap { margin-top: 12px; }
.mockup-sidebar-label { margin-bottom: 4px; }
```

- [ ] **Step 3: Add responsive rules for showcase**

Add inside the existing `@media (max-width: 640px)` block:

```css
  .showcase-inner {
    grid-template-columns: 1fr;
  }

  .showcase-inner--reversed {
    direction: ltr;
  }

  .showcase-inner .mockup {
    order: -1;
  }

  .showcase-title {
    font-size: 22px;
  }

  .mockup-obsidian .mockup-body {
    flex-direction: column;
  }

  .mockup-sidebar {
    width: 100%;
    border-right: none;
    border-bottom: 1px solid var(--border);
  }

  .mockup-preview {
    width: 100%;
  }

  .mockup-body {
    overflow-x: auto;
  }
```

- [ ] **Step 2: Commit**

```bash
git add css/style.css
git commit -m "style: add showcase/mockup CSS foundation"
```

---

### Task 2: Remove old sections, add showcases 1-2

**Files:**
- Modify: `index.html`
- Modify: `css/style.css`

- [ ] **Step 1: Remove old HTML sections from index.html**

Remove these blocks:

1. Features section (lines 187–229): the `<section class="section fade-in">` containing "What rwd extracts", plus its preceding divider (line 185) and following divider (line 231)
2. Output Example section (lines 299–337): the `<section class="section fade-in">` containing "From sessions to journal", plus its preceding divider (line 297) and following divider (line 339)

**Keep** the divider after Comparison section (line 183) — this becomes the boundary divider before showcase 1.

- [ ] **Step 2: Remove old CSS classes from style.css**

Remove these CSS blocks:

```
/* === Features === */ section: .features-grid, .feature-card, .feature-name, .feature-desc
/* === Output Example === */ section: .output-split, .output-panel, .output-after, .output-label, .output-code code, .output-arrow
```

Also remove `.features-grid`, `.output-split`, `.output-arrow` rules inside `@media (max-width: 640px)`.

Keep `.md-heading` and `.md-text` (reused in mockup notes).

- [ ] **Step 2: Add showcase 1 (Session Discovery) after Comparison section's closing divider**

Insert after the Comparison section's `</section>` (and its divider):

```html
    <!-- Showcase 1: Session Discovery -->
    <section class="showcase fade-in">
      <div class="showcase-inner">
        <div class="showcase-text">
          <span class="showcase-label">Session Discovery</span>
          <h2 class="showcase-title">One command. Every session.</h2>
          <p class="showcase-desc">
            Run <code>rwd today</code> and it scans all your AI coding sessions automatically. Claude Code, Codex — no manual export needed.
          </p>
        </div>
        <div class="mockup mockup-terminal">
          <div class="mockup-header">
            <div class="mockup-dots"><span></span><span></span><span></span></div>
            <span class="mockup-title">Terminal</span>
          </div>
          <div class="mockup-body">
            <div class="mockup-line"><span class="mc-dim">$</span> <span class="mc-primary">rwd today</span></div>
            <div class="mockup-line"><span class="mc-green">⠋</span> Discovering sessions...</div>
            <div class="mockup-line">  Found <span class="mc-primary">3</span> Claude Code sessions</div>
            <div class="mockup-line">  Found <span class="mc-primary">1</span> Codex session</div>
            <div class="mockup-line"><span class="mc-green">⠋</span> Planning execution...</div>
            <div class="mockup-line">  Strategy: single-shot (<span class="mc-primary">4</span> sessions, ~12k tokens)</div>
            <div class="mockup-line"><span class="mc-green">⠋</span> Analyzing with Claude API...</div>
          </div>
        </div>
      </div>
    </section>
```

- [ ] **Step 4: Add showcase 2 (Smart Extraction)**

Insert immediately after showcase 1:

```html
    <!-- Showcase 2: Smart Extraction -->
    <section class="showcase fade-in">
      <div class="showcase-inner showcase-inner--reversed">
        <div class="showcase-text">
          <span class="showcase-label">Smart Extraction</span>
          <h2 class="showcase-title">Finds what actually matters</h2>
          <p class="showcase-desc">
            Not a log dump. rwd identifies decisions you made, questions you had, mistakes the AI made, and things you learned.
          </p>
        </div>
        <div class="mockup mockup-note">
          <div class="mockup-header">
            <div class="mockup-dots"><span></span><span></span><span></span></div>
            <span class="mockup-title">2025-03-24.md</span>
          </div>
          <div class="mockup-body">
            <div class="md-heading">## Key Decisions</div>
            <div class="md-text">- <span class="mc-bold">Axum over Actix-web</span>: Simpler middleware API</div>
            <div class="md-heading">## Curiosities</div>
            <div class="md-text">- How does tower middleware chain work?</div>
            <div class="md-heading">## Corrections</div>
            <div class="md-text">- <span class="mc-muted">Model said:</span> Use .layer() for auth</div>
            <div class="md-text">  <span class="mc-green">Fix:</span> Must wrap in ServiceBuilder first</div>
            <div class="md-heading">## TIL</div>
            <div class="md-text">- Tower ServiceBuilder ordering matters</div>
          </div>
        </div>
      </div>
    </section>
```

- [ ] **Step 5: Commit**

```bash
git add index.html css/style.css
git commit -m "feat: replace feature/output sections with showcases 1-2"
```

---

### Task 3: HTML — Add showcases 3-4

**Files:**
- Modify: `index.html`

- [ ] **Step 1: Add showcase 3 (Privacy)**

Insert after showcase 2:

```html
    <!-- Showcase 3: Privacy -->
    <section class="showcase fade-in">
      <div class="showcase-inner">
        <div class="showcase-text">
          <span class="showcase-label">Privacy First</span>
          <h2 class="showcase-title">Sensitive data never leaves</h2>
          <p class="showcase-desc">
            API keys, AWS credentials, GitHub tokens, private IPs — automatically masked before LLM analysis. Eight patterns catch secrets you might miss.
          </p>
        </div>
        <div class="mockup mockup-code">
          <div class="mockup-header">
            <div class="mockup-dots"><span></span><span></span><span></span></div>
            <span class="mockup-title">Redactor</span>
          </div>
          <div class="mockup-body">
            <div class="mockup-line mc-dim">export OPENAI_KEY=sk-abc123def456ghi789...</div>
            <div class="mockup-line mc-dim">Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWI...</div>
            <div class="mockup-line mc-dim">Server: 192.168.1.42:3000</div>
            <div class="mockup-line mc-dim mockup-sep">↓</div>
            <div class="mockup-line">export OPENAI_KEY=<span class="mc-green">[REDACTED:API_KEY]</span></div>
            <div class="mockup-line"><span class="mc-green">[REDACTED:BEARER_TOKEN]</span></div>
            <div class="mockup-line">Server: <span class="mc-green">[REDACTED:PRIVATE_IP]</span>:3000</div>
            <div class="mockup-line mockup-gap"><span class="mc-green">✓</span> Redacted: API_KEY: 1, BEARER_TOKEN: 1, PRIVATE_IP: 1</div>
          </div>
        </div>
      </div>
    </section>
```

- [ ] **Step 2: Add showcase 4 (Daily Notes)**

Insert after showcase 3:

```html
    <!-- Showcase 4: Daily Notes -->
    <section class="showcase fade-in">
      <div class="showcase-inner showcase-inner--reversed">
        <div class="showcase-text">
          <span class="showcase-label">Daily Notes</span>
          <h2 class="showcase-title">Lands right in your vault</h2>
          <p class="showcase-desc">
            Structured markdown saved to your Obsidian vault or any directory. Each session becomes a documented chapter of your day.
          </p>
        </div>
        <div class="mockup mockup-obsidian">
          <div class="mockup-header">
            <div class="mockup-dots"><span></span><span></span><span></span></div>
            <span class="mockup-title">Obsidian</span>
          </div>
          <div class="mockup-body">
            <div class="mockup-sidebar">
              <div class="mc-muted mockup-sidebar-label">Daily</div>
              <div class="mockup-sidebar-item mockup-sidebar-item--active">2025-03-24.md</div>
              <div class="mockup-sidebar-item">2025-03-23.md</div>
              <div class="mockup-sidebar-item">2025-03-22.md</div>
            </div>
            <div class="mockup-preview">
              <div class="md-heading"># 2025-03-24 Dev Session Review</div>
              <div class="md-heading">## Claude Code</div>
              <div class="md-heading">### Work Summary</div>
              <div class="md-text">Implemented order validation for the e-commerce API...</div>
              <div class="md-heading">### Key Decisions</div>
              <div class="md-text">- <span class="mc-bold">Axum over Actix-web</span>: Simpler middleware API</div>
            </div>
          </div>
        </div>
      </div>
    </section>
```

- [ ] **Step 3: Commit**

```bash
git add index.html
git commit -m "feat: add showcase sections 3-4 (privacy, daily notes)"
```

---

### Task 4: HTML — Add showcases 5-6, wire dividers

**Files:**
- Modify: `index.html`

- [ ] **Step 1: Add showcase 5 (Work Summary)**

Insert after showcase 4:

```html
    <!-- Showcase 5: Work Summary -->
    <section class="showcase fade-in">
      <div class="showcase-inner">
        <div class="showcase-text">
          <span class="showcase-label">Work Summary</span>
          <h2 class="showcase-title">Ready for standup</h2>
          <p class="showcase-desc">
            <code>rwd summary</code> generates a concise work report grouped by project. Non-technical language, copied to clipboard — paste straight into your status update.
          </p>
        </div>
        <div class="mockup mockup-terminal">
          <div class="mockup-header">
            <div class="mockup-dots"><span></span><span></span><span></span></div>
            <span class="mockup-title">Terminal</span>
          </div>
          <div class="mockup-body">
            <div class="mockup-line"><span class="mc-dim">$</span> <span class="mc-primary">rwd summary</span></div>
            <div class="mockup-line">&nbsp;</div>
            <div class="mockup-line"><span class="mc-bold">### E-commerce API</span></div>
            <div class="mockup-line">- Order validation logic was completed</div>
            <div class="mockup-line">- Payment webhook handling was resolved</div>
            <div class="mockup-line">&nbsp;</div>
            <div class="mockup-line"><span class="mc-bold">### Internal Dashboard</span></div>
            <div class="mockup-line">- User analytics chart was implemented</div>
            <div class="mockup-line">&nbsp;</div>
            <div class="mockup-line"><span class="mc-green">✓</span> Copied to clipboard</div>
          </div>
        </div>
      </div>
    </section>
```

- [ ] **Step 2: Add showcase 6 (Slack Message)**

Insert after showcase 5:

```html
    <!-- Showcase 6: Slack Message -->
    <section class="showcase fade-in">
      <div class="showcase-inner showcase-inner--reversed">
        <div class="showcase-text">
          <span class="showcase-label">Slack Message</span>
          <h2 class="showcase-title">Share without the jargon</h2>
          <p class="showcase-desc">
            <code>rwd slack</code> turns your technical day into a team-friendly update. No file paths, no branch names — just clear outcomes ready to post.
          </p>
        </div>
        <div class="mockup mockup-terminal">
          <div class="mockup-header">
            <div class="mockup-dots"><span></span><span></span><span></span></div>
            <span class="mockup-title">Terminal</span>
          </div>
          <div class="mockup-body">
            <div class="mockup-line"><span class="mc-dim">$</span> <span class="mc-primary">rwd slack</span></div>
            <div class="mockup-line">&nbsp;</div>
            <div class="mockup-line"><span class="mc-bold">[Today's Work Update]</span></div>
            <div class="mockup-line">- Order validation feature was completed</div>
            <div class="mockup-line">- Payment processing issue was identified and fixed</div>
            <div class="mockup-line">- User analytics dashboard was implemented</div>
            <div class="mockup-line">&nbsp;</div>
            <div class="mockup-line"><span class="mc-green">✓</span> Copied to clipboard</div>
          </div>
        </div>
      </div>
    </section>
```

- [ ] **Step 3: Add boundary divider after showcase 6**

Add `<div class="divider"></div>` after showcase 6's `</section>`, before the Install section.

Final divider layout should be exactly:
1. `<div class="divider"></div>` — after Comparison, before showcase 1 (already exists from step 2)
2. No dividers between showcases 1–6
3. `<div class="divider"></div>` — after showcase 6, before Install (add this now)

- [ ] **Step 4: Commit**

```bash
git add index.html
git commit -m "feat: add showcase sections 5-6 (work summary, slack message)"
```

---

### Task 5: Visual verification

**Files:** none (read-only verification)

- [ ] **Step 1: Start dev server**

```bash
vercel dev
```

- [ ] **Step 2: Verify desktop layout**

Open `http://localhost:3000` in browser. Check:
- [ ] 6 showcase sections visible between Comparison and Install
- [ ] Odd sections (1, 3, 5): text left, mockup right
- [ ] Even sections (2, 4, 6): mockup left, text right
- [ ] Mockup frames have macOS chrome (dots + title)
- [ ] All 6 mockups render content correctly
- [ ] Obsidian mockup (section 4) shows sidebar + preview split
- [ ] Colors match spec (green accents, dim/primary contrast)
- [ ] Fade-in animation works on scroll
- [ ] Dividers only at showcase block boundaries (not between each showcase)

- [ ] **Step 3: Verify mobile layout**

Resize browser to < 640px width. Check:
- [ ] Single column layout
- [ ] Mockup appears above text in all sections
- [ ] Obsidian sidebar stacks vertically
- [ ] Text sizes reduced (showcase-title: 22px)

- [ ] **Step 4: Fix any issues found, commit**

```bash
git add -A
git commit -m "fix: address visual issues from showcase verification"
```
