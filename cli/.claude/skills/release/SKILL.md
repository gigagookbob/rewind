---
name: release
description: Bump the rwd version on a release branch and merge its PR so CI tags and publishes the release
user_invocable: true
---

# Release

Publish a new `rwd` version. The tag and the GitHub Release are created by CI when a
version bump reaches `main`; nothing is tagged or built locally.

How it works: `.github/workflows/release-on-main.yml` runs on every push to `main` that
touches `cli/src/**`, `cli/prompts/**`, `cli/Cargo.toml`, or `cli/Cargo.lock`. It reads the
version from `cli/Cargo.toml` and, if tag `vX.Y.Z` does not exist yet, calls `release.yml`,
which builds the macOS (arm64) and Linux (x86_64) binaries, tags the merge commit, and
publishes the release with generated notes. If the tag already exists it does nothing, so
merged changes stay unreleased until the version is bumped.

## Arguments

The user may give a bump type: `patch`, `minor`, or `major`.

Without one, choose from what merged since the last tag (`git log vA.B.C..origin/main`):
`minor` when a `feat` changes behavior, output format, config, or commands; otherwise
`patch`. Tell the user `current → new` and the reason, then continue. Ask only when the
choice is unclear.

## Steps

1. **Check preconditions**
   - The changes to release are already merged into `main`. Do not release unmerged work.
   - Working tree is clean (`git status --porcelain` is empty).
   - `git fetch origin`, then create `chore/release-vX.Y.Z` from `origin/main`. The branch
     name matters: `pr-labeler.yml` adds `skip-release-notes` to PRs from branches matching
     `chore/release-vX.Y.Z`, which keeps the bump PR out of the notes.
   - Tag `vX.Y.Z` must not exist (`git ls-remote --tags origin vX.Y.Z` prints nothing).

2. **Check release highlights before bumping**
   - Release notes are built from the bodies of the PRs merged since the last tag. Lines
     under a `## Release Highlights` heading in a PR body are copied into `Highlights`
     (format and rules: `cli/docs/RELEASES.md`).
   - If a merged PR changed setup, config, default behavior, or output and has no such
     section, add one to that PR's body now, in English, keeping the rest of the body as is.
     Bodies are read when the release runs, so this must happen before step 5.

3. **Apply the version bump** (in `cli/`)
   - Update `version` in `Cargo.toml`.
   - Run `cargo check` so `Cargo.lock` picks up the new version.
   - Only `Cargo.toml` and `Cargo.lock` change in this PR.

4. **Verify** (in `cli/`)
   - Run `/validate` (`cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
     `cargo test`) and confirm all pass.
   - `cargo run -- --version` prints `rwd X.Y.Z`.

5. **Open and merge the release PR**
   - Commit: `git add cli/Cargo.toml cli/Cargo.lock`, message `chore: release rwd vX.Y.Z`.
   - Push the branch and open a PR to `main` titled `chore: rwd vX.Y.Z 배포`. In the body,
     link the PRs this release ships and note that merging triggers the release.
   - Wait for CI (`gh pr checks <n> --watch`), then merge with a merge commit
     (`gh pr merge <n> --merge`). Invoking this skill is the request to push, open the PR,
     and merge it; do not merge with failing checks.

6. **Confirm the release**
   - Watch the `Release On Main` run for the merge commit
     (`gh run list --workflow "Release On Main" --limit 1`, then `gh run watch <id>`).
   - `gh release view vX.Y.Z` shows the release published with two assets:
     `rwd-aarch64-apple-darwin.tar.gz` and `rwd-x86_64-unknown-linux-gnu.tar.gz`.
   - Read the generated notes. Edit the release body by hand only in the cases listed
     under "When to Edit Manually" in `cli/docs/RELEASES.md`.
   - Report the release URL. Updating the locally installed binary (`rwd update`) is a
     separate action; run it only when the user asks.

## Rules

- Do NOT push to `main` directly, create the tag yourself, or create the GitHub Release
  manually — the workflow does all three from the merge commit.
- Do NOT attach local build artifacts to the release — CI builds them.
- Do NOT put other changes in the release PR.
- The notes cover every PR merged since the previous tag, including ones unrelated to
  the change that prompted the release. Mention this when reporting.
- If any step fails, stop and report. Do not proceed with partial state.
