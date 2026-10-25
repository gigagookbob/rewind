// Tests for generate_release_notes.cjs. Run from the repository root:
//   node --test .github/scripts/*.test.cjs
// Uses a fake `github` client and PR data only; no network access.

const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const test = require("node:test");

const generate = require("./generate_release_notes.cjs");
const { extractReleaseHighlights } = generate;

const ENV_KEYS = ["TAG_NAME", "TARGET_SHA", "PREVIOUS_TAG", "GITHUB_WORKSPACE"];

function mergeCommit(number) {
  return { commit: { message: `Merge pull request #${number} from example/branch\n\nbody` } };
}

// Runs the script against fake PRs (merged in the given order) and returns
// the written release-notes-intro.md text.
async function runGenerate(prs) {
  const workspace = fs.mkdtempSync(path.join(os.tmpdir(), "rwd-release-notes-"));
  const saved = Object.fromEntries(ENV_KEYS.map((key) => [key, process.env[key]]));
  Object.assign(process.env, {
    TAG_NAME: "v0.14.0",
    TARGET_SHA: "c182d2c0123456789abcdef",
    PREVIOUS_TAG: "v0.13.17",
    GITHUB_WORKSPACE: workspace,
  });

  const byNumber = new Map(prs.map((pr) => [pr.number, pr]));
  const github = {
    rest: {
      repos: {
        compareCommitsWithBasehead: async () => ({
          data: { commits: prs.map((pr) => mergeCommit(pr.number)) },
        }),
      },
      pulls: {
        get: async ({ pull_number }) => ({ data: byNumber.get(pull_number) }),
      },
    },
  };

  try {
    await generate({
      github,
      context: { repo: { owner: "gigagookbob", repo: "rewind" } },
      core: { notice: () => {} },
    });
    return fs.readFileSync(path.join(workspace, "release-notes-intro.md"), "utf8");
  } finally {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    fs.rmSync(workspace, { recursive: true, force: true });
  }
}

const FIXED_HIGHLIGHTS = [
  "- Compared against `v0.13.17` to keep the summary scoped to this release.",
  "- Tagged as `v0.14.0` from commit `c182d2c`.",
  "- Install with `cargo install --git https://github.com/gigagookbob/rewind.git` or update with `rwd update`.",
];

function highlightsSection(notes) {
  const lines = notes.split("\n");
  const start = lines.indexOf("### Highlights");
  const end = lines.indexOf("", start);
  return lines.slice(start + 1, end);
}

test("PRs without a Release Highlights section keep the current output", async () => {
  const notes = await runGenerate([
    {
      number: 6,
      title: "refactor: route sources through adapters",
      labels: [],
      body: "## Summary\n- internal\n",
    },
    { number: 7, title: "feat: add rwd today --dry-run", labels: [{ name: "enhancement" }], body: null },
    { number: 8, title: "fix(cli): check prompt hint", labels: [{ name: "bug" }], body: "" },
  ]);

  // Captured from the script before Release Highlights support was added.
  assert.equal(
    notes,
    [
      "## Release Notes",
      "",
      "This release includes 1 feature, 1 bug fix, 1 maintenance change across 3 merged PRs.",
      "",
      "### New Features",
      "- Add rwd today --dry-run (#7)",
      "",
      "### Highlights",
      ...FIXED_HIGHLIGHTS,
      "",
      "### Bug Fixes",
      "- Check prompt hint (#8)",
      "",
      "### Maintenance",
      "- Route sources through adapters (#6)",
      "",
    ].join("\n"),
  );
});

test("Release Highlights lines go before the fixed lines in PR order", async () => {
  const notes = await runGenerate([
    {
      number: 9,
      title: "feat: add Antigravity adapter",
      labels: [{ name: "enhancement" }],
      body: [
        "## Summary",
        "- Not a highlight",
        "",
        "## Release Highlights",
        "- `rwd today` now reads Antigravity conversations by default.",
        "- To opt out, add `enabled = false` under `[input.antigravity]`.",
        "",
        "## Test plan",
        "- Not a highlight either",
      ].join("\r\n"),
    },
    { number: 10, title: "fix: something", labels: [{ name: "bug" }], body: "## Summary\n- x\n" },
    {
      number: 11,
      title: "feat: add --dry-run",
      labels: [{ name: "enhancement" }],
      body: "## Release Highlights\n- Run `rwd today --dry-run` to preview sessions.\n",
    },
  ]);

  assert.deepEqual(highlightsSection(notes), [
    "- `rwd today` now reads Antigravity conversations by default.",
    "- To opt out, add `enabled = false` under `[input.antigravity]`.",
    "- Run `rwd today --dry-run` to preview sessions.",
    ...FIXED_HIGHLIGHTS,
  ]);
});

test("skip-release-notes PRs contribute no Release Highlights lines", async () => {
  const notes = await runGenerate([
    {
      number: 12,
      title: "chore: internal tweak",
      labels: [{ name: "skip-release-notes" }],
      body: "## Release Highlights\n- SKIPPED_MARKER should not appear.\n",
    },
    { number: 13, title: "docs: readme", labels: [{ name: "documentation" }], body: null },
  ]);

  assert.ok(!notes.includes("SKIPPED_MARKER"));
  assert.deepEqual(highlightsSection(notes), FIXED_HIGHLIGHTS);
});

test("extractReleaseHighlights keeps top-level bullets until the next heading", () => {
  const body = [
    "Intro",
    "```md",
    "## Release Highlights",
    "- FENCED_MARKER inside a code block",
    "```",
    "## Release Highlights  ",
    "- First line  ",
    "  - nested detail is not a release line",
    "* other bullet styles are ignored",
    "-",
    "plain text is ignored",
    "- Second line",
    "### Details",
    "- after the next heading",
  ].join("\n");

  assert.deepEqual(extractReleaseHighlights(body), ["- First line", "- Second line"]);
  assert.deepEqual(extractReleaseHighlights(""), []);
  assert.deepEqual(extractReleaseHighlights(null), []);
});
