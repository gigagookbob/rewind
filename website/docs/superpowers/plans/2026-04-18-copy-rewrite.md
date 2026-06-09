# Landing Page Copy Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite every user-facing string on `index.html` to strip AI-generated voice, replace six repetitive Showcase sections with four Feature cards, remove the Comparison section, and add a Maker's Note section. No visual system changes.

**Architecture:** Static HTML + CSS site, no framework, no build step. All edits land in `index.html`, three CSS files, and `sitemap.xml`. No JavaScript or image changes. Verification is grep checks plus browser sanity at 640px.

**Tech Stack:** HTML5, CSS3, Prettier (formatting only). No Node, no build.

**Spec:** `docs/superpowers/specs/2026-04-18-copy-rewrite-design.md`

**Environment note:** Run all commands from a WSL shell (Ubuntu). Running from Windows bash over `\\wsl.localhost\` UNC paths can surface intermittent I/O errors on certain files.

---

## File Structure

| File | Responsibility | Change type |
|---|---|---|
| `index.html` | All copy, meta tags, JSON-LD, section markup | Modify |
| `css/03-components.css` | Component styles — add `.feature-*`, `.maker-note*`, `.footer-cta-sub`; remove `.comparison-*` | Modify |
| `css/04-showcase.css` | Showcase and Mockup styles | Delete (after Showcases are removed) |
| `css/06-responsive.css` | 640px breakpoint — remove `.showcase-*` / `.mockup-*` rules, add `.feature-grid` mobile stack | Modify |
| `css/02-layout.css` | Layout helpers — remove `.showcase-inner` from grouped selector | Modify |
| `sitemap.xml` | `<lastmod>` bump | Modify |

No new files are created. `css/04-showcase.css` is removed after Task 12.

---

## Task 1: Update meta tags, OG, Twitter

**Files:**
- Modify: `index.html` lines 6–55 (head section, title + meta + OG + Twitter)

- [ ] **Step 1: Replace HTML `<title>`**

In `index.html` line 6, replace:
```html
<title>rwd: Rewind AI Coding Sessions into a Daily Developer Journal</title>
```
with:
```html
<title>rwd: Rewind your day. Daily notes for Claude Code, Codex, and other AI agents.</title>
```

- [ ] **Step 2: Replace `<meta name="description">`**

Replace lines 7–10 with:
```html
<meta
  name="description"
  content="Open-source CLI for AI agents like Claude Code and Codex. Turns your coding day into one daily note of decisions, questions, and corrections."
/>
```

- [ ] **Step 3: Replace Open Graph `og:title`**

Replace lines 19–22 with:
```html
<meta
  property="og:title"
  content="rwd: Rewind your day. Daily notes for Claude Code, Codex, and other AI agents."
/>
```

- [ ] **Step 4: Replace Open Graph `og:description`**

Replace lines 23–26 with:
```html
<meta
  property="og:description"
  content="Open-source CLI for AI agents like Claude Code and Codex. Turns your coding day into one daily note of decisions, questions, and corrections."
/>
```

- [ ] **Step 5: Replace `og:image:alt`**

Replace lines 35–38 with:
```html
<meta
  property="og:image:alt"
  content="rwd, an open-source CLI that turns your AI agent sessions into a daily note."
/>
```

- [ ] **Step 6: Replace Twitter tags**

Replace lines 43–55 with:
```html
<meta
  name="twitter:title"
  content="rwd: Rewind your day. Daily notes for Claude Code, Codex, and other AI agents."
/>
<meta
  name="twitter:description"
  content="Open-source CLI for AI agents like Claude Code and Codex. Turns your coding day into one daily note of decisions, questions, and corrections."
/>
<meta name="twitter:image" content="https://www.rewind.day/assets/og-image.png" />
<meta
  name="twitter:image:alt"
  content="rwd, an open-source CLI that turns your AI agent sessions into a daily note."
/>
```

- [ ] **Step 7: Verify no em-dash was introduced**

Run:
```bash
grep -n "—" index.html | head -20
```
Expected: no new em-dash output beyond any that still exist elsewhere in the file. (Task 12 will strip all remaining em-dashes; for now just verify this task did not add any.)

- [ ] **Step 8: Commit**

```bash
git add index.html
git commit -m "content: rewrite meta tags and OG for AI agents positioning"
```

---

## Task 2: Update JSON-LD (WebSite, SoftwareApplication, FAQPage)

**Files:**
- Modify: `index.html` lines 58–146 (JSON-LD script block)

- [ ] **Step 1: Update `WebSite.description`**

In the JSON-LD block, replace:
```json
"description": "Open-source CLI for Claude Code and Codex that rewinds AI coding sessions into daily journals with key decisions, model corrections, summaries, and shareable updates."
```
(appears inside the `WebSite` node) with:
```json
"description": "Open-source CLI for AI agents like Claude Code and Codex. Turns your coding day into one daily note of decisions, questions, and corrections."
```

- [ ] **Step 2: Update `SoftwareApplication.description`**

In the `SoftwareApplication` node, replace the description field with the same string as Step 1.

- [ ] **Step 3: Rewrite `SoftwareApplication.featureList`**

Replace the existing `featureList` array with:
```json
"featureList": [
  "Capture sessions from Claude Code and Codex automatically",
  "Keep decisions, questions, and corrections in a daily Markdown note",
  "Mask API keys, tokens, and private IPs locally before analysis",
  "Generate shareable summaries and Slack-ready posts"
]
```

- [ ] **Step 4: Rewrite `FAQPage.mainEntity`**

Replace the entire `FAQPage.mainEntity` array with exactly these five entries:
```json
"mainEntity": [
  {
    "@type": "Question",
    "name": "Which AI agents does rwd read?",
    "acceptedAnswer": {
      "@type": "Answer",
      "text": "Claude Code and Codex today. More CLIs on the way."
    }
  },
  {
    "@type": "Question",
    "name": "Where do the notes go?",
    "acceptedAnswer": {
      "@type": "Answer",
      "text": "Any folder you choose, as Markdown. Works well with Obsidian but doesn't require it."
    }
  },
  {
    "@type": "Question",
    "name": "English or Korean output?",
    "acceptedAnswer": {
      "@type": "Answer",
      "text": "Both. Add --lang en or --lang ko to today, summary, or slack."
    }
  },
  {
    "@type": "Question",
    "name": "What does it cost?",
    "acceptedAnswer": {
      "@type": "Answer",
      "text": "rwd is free and MIT licensed. You bring your own API key for whichever model it uses."
    }
  },
  {
    "@type": "Question",
    "name": "Is my data sent raw to the model?",
    "acceptedAnswer": {
      "@type": "Answer",
      "text": "No. API keys, bearer tokens, and private IPs are masked locally before anything is sent."
    }
  }
]
```

- [ ] **Step 5: Verify JSON-LD parses**

Run:
```bash
python3 -c "import json,re,sys; html=open('index.html').read(); m=re.search(r'<script type=\"application/ld\\+json\">(.*?)</script>', html, re.S); json.loads(m.group(1)); print('OK')"
```
Expected: `OK`. If `json.loads` raises, fix the syntax (most likely a trailing comma or missing brace).

- [ ] **Step 6: Verify FAQ entry count equals five**

Run:
```bash
python3 -c "import json,re; html=open('index.html').read(); m=re.search(r'<script type=\"application/ld\\+json\">(.*?)</script>', html, re.S); data=json.loads(m.group(1)); faq=[n for n in data['@graph'] if n.get('@type')=='FAQPage'][0]; print(len(faq['mainEntity']))"
```
Expected: `5`

- [ ] **Step 7: Commit**

```bash
git add index.html
git commit -m "content: rewrite JSON-LD description, featureList, and FAQ entries"
```

---

## Task 3: Rewrite Hero section

**Files:**
- Modify: `index.html` lines 227–249 (hero section)

- [ ] **Step 1: Replace hero headline**

In `index.html`, replace:
```html
<h1 class="hero-title">Your AI coding sessions, journaled by day.</h1>
```
with:
```html
<h1 class="hero-title">Rewind your day.</h1>
```

- [ ] **Step 2: Replace hero subtitle**

Replace the `<p class="hero-subtitle">` block:
```html
<p class="hero-subtitle">
  rwd is an open-source CLI for Claude Code and Codex that rewinds each session into a
  daily journal with work summaries, key decisions, model corrections, and shareable
  updates.
</p>
```
with:
```html
<p class="hero-subtitle">
  Decisions. Questions. Corrections. Everything you worked out with your AI agents today, stitched into one daily note.
</p>
```

- [ ] **Step 3: Replace primary CTA label**

Replace:
```html
<a href="#install" class="btn btn-primary">Install</a>
```
with:
```html
<a href="#install" class="btn btn-primary">Install rwd</a>
```

- [ ] **Step 4: Verify visually**

Open `index.html` in a browser. Confirm the hero shows:
- Headline: `Rewind your day.`
- Subtitle with `Decisions. Questions. Corrections. Everything you worked out with your AI agents today, stitched into one daily note.`
- Primary button: `Install rwd`
- Secondary button: `View on GitHub` (unchanged)

- [ ] **Step 5: Commit**

```bash
git add index.html
git commit -m "content: rewrite hero headline, subtitle, and CTA label"
```

---

## Task 4: Update Terminal Demo alt text

**Files:**
- Modify: `index.html` line 266 (terminal image alt)

- [ ] **Step 1: Replace alt attribute**

Replace:
```html
alt="rwd CLI output showing session discovery, key decisions, and daily journal notes"
```
with:
```html
alt="rwd CLI output showing session discovery, decisions, and a daily note"
```

- [ ] **Step 2: Commit**

```bash
git add index.html
git commit -m "content: update terminal screenshot alt text"
```

---

## Task 5: Replace Comparison + Showcases with Feature cards

**Files:**
- Modify: `index.html` lines 282–537 (delete Comparison + Showcases 1–6, insert Feature cards)
- Modify: `css/03-components.css` (add `.feature-*` rules, remove `.comparison-*` rules)

- [ ] **Step 1: Remove Comparison section markup**

In `index.html`, delete everything from `<!-- Comparison -->` (around line 281) through the closing `</section>` and the following `<div class="divider"></div>` (around line 308–309). Before:
```html
<!-- Comparison -->
<section class="section fade-in">
  <div class="container">
    ...comparison card markup...
  </div>
</section>

<div class="divider"></div>
```
After deleting, the previous `<div class="divider"></div>` (around line 279) should be directly followed by the next showcase. We will remove those showcases in Step 2.

- [ ] **Step 2: Remove Showcase 1–6 markup**

Delete everything from `<!-- Showcase 1: Session Capture -->` through the closing `</section>` of Showcase 6 (`<!-- Showcase 6: Slack Handoff -->`). That removes six `<section class="showcase fade-in">` blocks. The `<div class="divider"></div>` right after Showcase 6 stays (it precedes the Install section).

- [ ] **Step 3: Insert Feature cards markup**

At the location where Comparison used to be (between the `<div class="divider"></div>` after Terminal Demo and the `<div class="divider"></div>` before Install), insert:

```html
<!-- Feature cards -->
<section class="section fade-in">
  <div class="container">
    <div class="feature-grid">
      <article class="feature-card">
        <h3 class="feature-title">Capture every session automatically</h3>
        <p class="feature-desc">
          rwd scans your AI agent sessions across projects and turns a day into one structured note. No copy-paste.
        </p>
        <p class="feature-note">Claude Code and Codex today. More agents on the way.</p>
      </article>
      <article class="feature-card">
        <h3 class="feature-title">Rewind the decisions you made</h3>
        <p class="feature-desc">
          Each note keeps what you decided, what you wondered, what you corrected. The reasoning trail AI agents strip out.
        </p>
      </article>
      <article class="feature-card">
        <h3 class="feature-title">Redact before analysis</h3>
        <p class="feature-desc">
          Sensitive values are masked locally before anything touches an LLM. API keys, tokens, private IPs stay yours.
        </p>
      </article>
      <article class="feature-card">
        <h3 class="feature-title">Share without rewriting</h3>
        <p class="feature-desc">
          <code>rwd summary</code> gives you a progress report. <code>rwd slack</code> gives you a team-ready post. Copy-to-clipboard, done.
        </p>
      </article>
    </div>
  </div>
</section>
```

- [ ] **Step 4: Add Feature card CSS rules**

In `css/03-components.css`, find the `/* === Comparison === */` block (lines 195–229). Replace the entire Comparison block with:

```css
/* === Feature cards === */
.feature-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.feature-card {
  background: var(--bg-secondary);
  border-radius: 10px;
  padding: 24px;
  border: 1px solid var(--border);
}

.feature-title {
  font-size: 18px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: -0.3px;
  line-height: 1.3;
  margin: 0 0 10px;
}

.feature-desc {
  font-size: 14px;
  color: var(--text-secondary);
  line-height: 1.6;
  margin: 0;
}

.feature-desc code {
  font-family: var(--font-mono);
  font-size: 13px;
  background: var(--bg-primary);
  padding: 2px 6px;
  border-radius: 4px;
}

.feature-note {
  margin: 12px 0 0;
  font-size: 12px;
  color: var(--text-muted);
}
```

- [ ] **Step 5: Verify HTML has no `comparison-` or `showcase-` references**

Run:
```bash
grep -nE "comparison-|showcase-" index.html
```
Expected: no output.

- [ ] **Step 6: Verify page in browser**

Open `index.html` in a browser. Confirm:
- Between Terminal Demo and Install you now see a 2×2 grid of four feature cards.
- No Comparison cards remain.
- No alternating showcase blocks with mockups remain.

- [ ] **Step 7: Commit**

```bash
git add index.html css/03-components.css
git commit -m "feat: replace showcases and comparison with four feature cards"
```

---

## Task 6: Add Maker's Note section

**Files:**
- Modify: `index.html` (insert section between Feature cards and Install)
- Modify: `css/03-components.css` (append `.maker-note*` rules)

- [ ] **Step 1: Insert Maker's Note markup**

In `index.html`, find the `<div class="divider"></div>` that sits between the Feature cards section (added in Task 5) and `<!-- Install -->`. We want the final order: Feature cards → new divider → Maker's Note → existing divider → Install.

Insert the following block IMMEDIATELY BEFORE that existing divider:

```html
<div class="divider"></div>

<!-- Maker's Note -->
<section class="section fade-in">
  <div class="container">
    <div class="maker-note">
      <p class="maker-note-body">
        I code with Claude every day. The code ends up in git. My decisions, my corrections, the questions I was working through don't. I wanted to keep them, so I wrote rwd.
      </p>
      <p class="maker-note-sign">gigagookbob</p>
    </div>
  </div>
</section>
```

After the insertion the structure reads: Feature cards section → new `<div class="divider"></div>` → Maker's Note section → existing `<div class="divider"></div>` → Install section.

- [ ] **Step 2: Append Maker's Note CSS**

Append to the end of `css/03-components.css`:

```css
/* === Maker's Note === */
.maker-note {
  max-width: 620px;
  margin: 0 auto;
  text-align: center;
}

.maker-note-body {
  font-style: italic;
  font-size: 16px;
  line-height: 1.7;
  color: var(--text-secondary);
  margin: 0;
}

.maker-note-sign {
  margin-top: 16px;
  font-size: 13px;
  color: var(--text-muted);
}
```

- [ ] **Step 3: Verify in browser**

Open `index.html`. Confirm:
- A centered italic paragraph appears between the Feature cards and the Install section.
- A signature `gigagookbob` sits below the paragraph.
- The section respects the dark theme (light text on near-black background).

- [ ] **Step 4: Commit**

```bash
git add index.html css/03-components.css
git commit -m "feat: add maker's note section"
```

---

## Task 7: Update Install section

**Files:**
- Modify: `index.html` lines around 542–572 (Install section)

- [ ] **Step 1: Replace section title**

Replace:
```html
<h2 class="section-title">Install in seconds</h2>
```
with:
```html
<h2 class="section-title">Install</h2>
```

- [ ] **Step 2: Shorten install note**

Replace:
```html
<p class="install-note">
  Native Windows is not supported. Please use
  <a href="https://learn.microsoft.com/en-us/windows/wsl/install" target="_blank" rel="noopener"
    ><strong><u>WSL2</u></strong></a
  >.
</p>
```
with:
```html
<p class="install-note">
  Native Windows is not supported. Use
  <a href="https://learn.microsoft.com/en-us/windows/wsl/install" target="_blank" rel="noopener"
    ><strong><u>WSL2</u></strong></a
  >.
</p>
```

(Only change: `Please use` → `Use`.)

- [ ] **Step 3: Commit**

```bash
git add index.html
git commit -m "content: tighten install title and note copy"
```

---

## Task 8: Rewrite FAQ HTML (5 items)

**Files:**
- Modify: `index.html` lines around 577–647 (FAQ section)

- [ ] **Step 1: Replace the entire FAQ item list**

Find the `<div class="faq-list">` block inside the FAQ section. Replace ALL contents (six existing faq-item blocks) with exactly these five faq-item blocks:

```html
<div class="faq-item">
  <button class="faq-question" aria-expanded="false">
    <span>Which AI agents does rwd read?</span>
    <span class="faq-icon">+</span>
  </button>
  <div class="faq-answer">
    <p>Claude Code and Codex today. More CLIs on the way.</p>
  </div>
</div>
<div class="faq-item">
  <button class="faq-question" aria-expanded="false">
    <span>Where do the notes go?</span>
    <span class="faq-icon">+</span>
  </button>
  <div class="faq-answer">
    <p>Any folder you choose, as Markdown. Works well with Obsidian but doesn't require it.</p>
  </div>
</div>
<div class="faq-item">
  <button class="faq-question" aria-expanded="false">
    <span>English or Korean output?</span>
    <span class="faq-icon">+</span>
  </button>
  <div class="faq-answer">
    <p>Both. Add <code>--lang en</code> or <code>--lang ko</code> to <code>today</code>, <code>summary</code>, or <code>slack</code>.</p>
  </div>
</div>
<div class="faq-item">
  <button class="faq-question" aria-expanded="false">
    <span>What does it cost?</span>
    <span class="faq-icon">+</span>
  </button>
  <div class="faq-answer">
    <p>rwd is free and MIT licensed. You bring your own API key for whichever model it uses.</p>
  </div>
</div>
<div class="faq-item">
  <button class="faq-question" aria-expanded="false">
    <span>Is my data sent raw to the model?</span>
    <span class="faq-icon">+</span>
  </button>
  <div class="faq-answer">
    <p>No. API keys, bearer tokens, and private IPs are masked locally before anything is sent.</p>
  </div>
</div>
```

- [ ] **Step 2: Verify exactly five FAQ items**

Run:
```bash
grep -c 'class="faq-item"' index.html
```
Expected: `5`

- [ ] **Step 3: Verify FAQ accordion still works**

Open `index.html` in a browser. Click each FAQ question. Confirm:
- Clicking expands the answer.
- Clicking again collapses it.
- Only one item is open at a time (existing behavior in `js/main.js`).

- [ ] **Step 4: Commit**

```bash
git add index.html
git commit -m "content: rewrite FAQ to five items with tighter copy"
```

---

## Task 9: Update Footer CTA

**Files:**
- Modify: `index.html` lines around 651–658 (Footer CTA section)
- Modify: `css/03-components.css` (append `.footer-cta-sub` rule)

- [ ] **Step 1: Replace Footer CTA title and add sub-line**

Replace the Footer CTA section:
```html
<!-- Footer CTA -->
<section class="footer-cta">
  <div class="container">
    <h2 class="footer-cta-title">Rewind today. Keep the reasoning.</h2>
    <div class="hero-cta">
      <a href="#install" class="btn btn-primary">Install rwd</a>
    </div>
  </div>
</section>
```
with:
```html
<!-- Footer CTA -->
<section class="footer-cta">
  <div class="container">
    <h2 class="footer-cta-title">Rewind your day.</h2>
    <p class="footer-cta-sub">Keep every decision you make.</p>
    <div class="hero-cta">
      <a href="#install" class="btn btn-primary">Install rwd</a>
    </div>
  </div>
</section>
```

- [ ] **Step 2: Append Footer CTA sub-line CSS**

In `css/03-components.css`, find the `/* === Footer CTA === */` block. Add the following rule inside that section (after `.footer-cta-title`):

```css
.footer-cta-sub {
  font-size: 15px;
  color: var(--text-muted);
  margin-bottom: 24px;
}
```

Then change the `.footer-cta-title` `margin-bottom` from `24px` to `8px` so the title sits closer to the sub-line:

```css
.footer-cta-title {
  font-size: 32px;
  font-weight: 600;
  color: var(--text-primary);
  letter-spacing: -0.5px;
  margin-bottom: 8px;
}
```

- [ ] **Step 3: Verify in browser**

Open `index.html`. Confirm the Footer CTA renders in this order:
1. Large title `Rewind your day.`
2. Muted sub-line `Keep every decision you make.` directly below
3. Install rwd button

- [ ] **Step 4: Commit**

```bash
git add index.html css/03-components.css
git commit -m "feat: update footer CTA copy and add sub-line"
```

---

## Task 10: Fix footer bottom typo

**Files:**
- Modify: `index.html` line around 695

- [ ] **Step 1: Fix `Wolrd` typo**

Replace:
```html
<span>&copy; 2026 GookbobWolrd Inc. All rights reserved.</span>
```
with:
```html
<span>&copy; 2026 GookbobWorld Inc. All rights reserved.</span>
```

- [ ] **Step 2: Verify**

Run:
```bash
grep -n "Wolrd\|GookbobWorld" index.html
```
Expected: only one line containing `GookbobWorld`, zero lines containing `Wolrd`.

- [ ] **Step 3: Commit**

```bash
git add index.html
git commit -m "fix: correct Wolrd typo in footer copyright"
```

---

## Task 11: Remove unused Showcase/Mockup/Comparison CSS

**Files:**
- Delete: `css/04-showcase.css` (entire file)
- Modify: `index.html` line 152 (remove `<link>` reference to 04-showcase.css)
- Modify: `css/06-responsive.css` (remove `.showcase-*`, `.mockup-*` rules)
- Modify: `css/02-layout.css` (remove `.showcase-inner` from grouped selector)

- [ ] **Step 1: Confirm no HTML references to removed classes remain**

Run:
```bash
grep -nE "showcase-|mockup-|comparison-|md-heading|md-text|mc-dim|mc-primary|mc-green|mc-bold|mc-muted" index.html
```
Expected: no output. If there are matches, the Task 5 HTML replacement missed something. Stop and fix Task 5 first.

- [ ] **Step 2: Remove the 04-showcase.css link from `index.html`**

Replace:
```html
<link rel="stylesheet" href="css/02-layout.css?v=3" />
<link rel="stylesheet" href="css/03-components.css?v=3" />
<link rel="stylesheet" href="css/04-showcase.css?v=3" />
<link rel="stylesheet" href="css/05-utilities.css?v=3" />
```
with:
```html
<link rel="stylesheet" href="css/02-layout.css?v=3" />
<link rel="stylesheet" href="css/03-components.css?v=3" />
<link rel="stylesheet" href="css/05-utilities.css?v=3" />
```

- [ ] **Step 3: Delete `css/04-showcase.css`**

Run:
```bash
git rm css/04-showcase.css
```

- [ ] **Step 4: Trim `css/02-layout.css` grouped selector**

In `css/02-layout.css`, replace:
```css
section > .container,
section > .showcase-inner,
footer > .container {
  margin-left: 320px;
  margin-right: 320px;
}
```
with:
```css
section > .container,
footer > .container {
  margin-left: 320px;
  margin-right: 320px;
}
```

- [ ] **Step 5: Trim `css/06-responsive.css`**

In `css/06-responsive.css`, replace lines 3–8:
```css
section > .container,
section > .showcase-inner,
footer > .container {
  margin-left: 0;
  margin-right: 0;
}
```
with:
```css
section > .container,
footer > .container {
  margin-left: 0;
  margin-right: 0;
}
```

Then delete the entire `.showcase-*` and `.mockup-*` blocks in the same file (lines 64–96 in the current version):
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
Delete all of the above from `css/06-responsive.css`.

Also delete the `.comparison-grid` block in `css/06-responsive.css`:
```css
.comparison-grid {
  grid-template-columns: 1fr;
}
```

- [ ] **Step 6: Add Feature grid mobile rule**

In `css/06-responsive.css`, in the `@media (max-width: 640px)` block (where `.comparison-grid` used to be), add:
```css
.feature-grid {
  grid-template-columns: 1fr;
}
```

- [ ] **Step 7: Verify page still renders at desktop and 640px**

Open `index.html` in a browser. Resize to 640px and narrower. Confirm:
- Nav, Hero, Terminal, Feature cards, Maker's Note, Install, FAQ, Footer CTA, Footer all render without horizontal scroll.
- At 640px the Feature grid collapses to a single column.
- No visual artifacts from removed styles.

- [ ] **Step 8: Verify CSS files contain no references to removed classes**

Run:
```bash
grep -rnE "showcase-|mockup-|comparison-|md-heading|md-text" css/
```
Expected: no output (empty).

- [ ] **Step 9: Commit**

```bash
git add index.html css/02-layout.css css/06-responsive.css
git commit -m "chore: remove unused showcase, mockup, and comparison CSS"
```

(The `git rm` from Step 3 stages the deletion; this commit includes it.)

---

## Task 12: Strip any remaining em-dashes and forbidden words

**Files:**
- Modify: `index.html` (if grep reveals leftover em-dash or banned words)

- [ ] **Step 1: Grep for em-dash**

Run:
```bash
grep -n "—" index.html
```
Expected: no output. If any em-dash remains, replace it with a period, comma, or line break depending on context.

- [ ] **Step 2: Grep for forbidden marketing words**

Run:
```bash
grep -inE "seamlessly|effortlessly|supercharge|transform|unlock|elevate" index.html
```
Expected: no output. If any match, rewrite the sentence.

- [ ] **Step 3: Grep for "AI coding" leftover (should be "AI agents")**

Run:
```bash
grep -n "AI coding" index.html
```
Expected: no output. Every mention of AI should be `AI agent` / `AI agents` per the spec. If any `AI coding` remains, replace it with `AI agent` / `AI agents` as context requires.

- [ ] **Step 4: Commit (only if any edits were needed in this task)**

If any fixes landed, run:
```bash
git add index.html
git commit -m "content: strip remaining em-dashes and forbidden words"
```
If the three greps returned empty on first run, skip this commit.

---

## Task 13: Update sitemap lastmod

**Files:**
- Modify: `sitemap.xml` line 5

- [ ] **Step 1: Bump `<lastmod>`**

Replace:
```xml
<lastmod>2026-04-11</lastmod>
```
with:
```xml
<lastmod>2026-04-18</lastmod>
```

- [ ] **Step 2: Commit**

```bash
git add sitemap.xml
git commit -m "chore: bump sitemap lastmod"
```

---

## Task 14: Final verification against acceptance criteria

No file changes in this task — it is a checklist that proves the spec is met.

- [ ] **Step 1: Section count equals seven inside `<main>`**

Run:
```bash
python3 -c "import re; html=open('index.html').read(); main=re.search(r'<main>(.*?)</main>', html, re.S).group(1); print(len(re.findall(r'<section\b', main)))"
```
Expected: `7` (Hero, Terminal, Feature cards, Maker's Note, Install, FAQ, Footer CTA).

- [ ] **Step 2: No em-dashes anywhere in the page**

Run:
```bash
grep -n "—" index.html
```
Expected: no output.

- [ ] **Step 3: No forbidden marketing words**

Run:
```bash
grep -inE "seamlessly|effortlessly|supercharge|transform|unlock|elevate" index.html
```
Expected: no output.

- [ ] **Step 4: FAQ count matches JSON-LD count**

Run:
```bash
grep -c 'class="faq-item"' index.html
python3 -c "import json,re; html=open('index.html').read(); m=re.search(r'<script type=\"application/ld\\+json\">(.*?)</script>', html, re.S); data=json.loads(m.group(1)); faq=[n for n in data['@graph'] if n.get('@type')=='FAQPage'][0]; print(len(faq['mainEntity']))"
```
Expected: both print `5`.

- [ ] **Step 5: Manual browser check at desktop and 640px**

Open `index.html`. Check:
- Hero headline reads `Rewind your day.`
- Four Feature cards render in a 2×2 grid at desktop, single column at 640px.
- Maker's Note renders between Feature cards and Install with italic text centered.
- Footer CTA shows the new title + sub-line + button.
- No visual glitches (no oversized text, no overflow, no missing borders).
- FAQ accordion expands and collapses.
- Copy button on Install command copies the command (existing behavior).

- [ ] **Step 6: Confirm no unexpected file changes**

Run:
```bash
git status
```
Expected: clean working tree with no unstaged changes other than any from unrelated work (for example, the `CLAUDE.md` modification that pre-existed this plan).

- [ ] **Step 7: Confirm branch is ready to merge**

Run:
```bash
git log --oneline $(git merge-base HEAD main)..HEAD
```
Expected: a clean sequence of the commits created by this plan. If running on `main` directly, consider creating a feature branch retroactively with `git branch copy-rewrite`.

---

## Self-Review

**Spec coverage**

| Spec section | Covered by |
|---|---|
| 3 Copy style rules | Tasks 12 and 14 verification greps |
| 4 Structural changes (Comparison removed, Showcases → Feature cards) | Task 5 |
| 4 Structural changes (Maker's Note added) | Task 6 |
| 5.1 Nav (unchanged) | No task needed |
| 5.2 Hero | Task 3 |
| 5.3 Terminal Demo alt | Task 4 |
| 5.4 Feature cards | Task 5 |
| 5.5 Maker's Note | Task 6 |
| 5.6 Install | Task 7 |
| 5.7 FAQ | Task 8 (visible) + Task 2 (JSON-LD) |
| 5.8 Footer CTA | Task 9 |
| 5.9 Footer bottom typo | Task 10 |
| 5.10 Meta tags + JSON-LD | Task 1 + Task 2 |
| 6 Scope boundaries (CSS adds + deletions) | Tasks 5, 6, 9, 11 |
| 7 Implementation order | Tasks 1–13 follow the order |
| 8 Acceptance criteria | Task 14 |

All spec sections are covered.

**Placeholder scan**

No `TBD`, `TODO`, "implement later", "add appropriate X" strings appear in the plan. Every step has the exact HTML or CSS to write, exact file path, and an explicit verification command where applicable.

**Type / name consistency**

- Feature card classes: `.feature-grid`, `.feature-card`, `.feature-title`, `.feature-desc`, `.feature-note`. Consistent across Tasks 5 and 11.
- Maker's Note classes: `.maker-note`, `.maker-note-body`, `.maker-note-sign`. Consistent in Task 6.
- Footer CTA classes: `.footer-cta`, `.footer-cta-title`, `.footer-cta-sub`. Consistent in Task 9.
- Commit message types follow existing repo style (`content:`, `feat:`, `fix:`, `chore:`). `content:` is slightly non-standard; if the repo owner objects, swap for `refactor:` or `style:`.
