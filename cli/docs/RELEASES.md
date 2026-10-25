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
