const fs = require("node:fs");
const path = require("node:path");

function pluralize(count, noun) {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

function cleanTitle(title) {
  const withoutPrefix = title.replace(/^\w+(?:\([^)]+\))?!?:\s*/, "");
  return withoutPrefix.charAt(0).toUpperCase() + withoutPrefix.slice(1);
}

function extractPrNumbers(commits) {
  const numbers = new Set();

  for (const commit of commits) {
    const subject = commit.commit.message.split("\n")[0];
    const match =
      subject.match(/Merge pull request #(\d+)/) ??
      subject.match(/\(#(\d+)\)$/);

    if (match) {
      numbers.add(Number(match[1]));
    }
  }

  return [...numbers];
}

// Collects the top-level "- " lines under a "## Release Highlights" heading in
// a PR body, up to the next heading. Lines inside code fences are ignored.
function extractReleaseHighlights(body) {
  const highlights = [];
  let inSection = false;
  let inFence = false;

  for (const rawLine of (body ?? "").split(/\r?\n/)) {
    const line = rawLine.trimEnd();
    if (line.trimStart().startsWith("```")) {
      inFence = !inFence;
      continue;
    }
    if (inFence) continue;

    if (/^#{1,6}\s/.test(line)) {
      inSection = /^##\s+Release Highlights$/.test(line);
      continue;
    }
    if (inSection && line.startsWith("- ") && line.slice(2).trim()) {
      highlights.push(line);
    }
  }

  return highlights;
}

function bucketFor(pr) {
  const labels = new Set(pr.labels.map((label) => label.name));

  if (labels.has("enhancement")) return "New Features";
  if (labels.has("bug")) return "Bug Fixes";
  if (labels.has("performance")) return "Performance";
  if (labels.has("documentation")) return "Documentation";
  return "Maintenance";
}

module.exports = async ({ github, context, core }) => {
  const owner = context.repo.owner;
  const repo = context.repo.repo;
  const tagName = process.env.TAG_NAME;
  const targetSha = process.env.TARGET_SHA;
  const previousTag = process.env.PREVIOUS_TAG;
  const outputPath = path.join(process.env.GITHUB_WORKSPACE, "release-notes-intro.md");
  const categories = new Map([
    ["New Features", []],
    ["Bug Fixes", []],
    ["Performance", []],
    ["Documentation", []],
    ["Maintenance", []],
  ]);
  const summaryNouns = {
    "New Features": "feature",
    "Bug Fixes": "bug fix",
    Performance: "performance update",
    Documentation: "documentation update",
    Maintenance: "maintenance change",
  };

  let commits = [];
  if (previousTag) {
    const { data } = await github.rest.repos.compareCommitsWithBasehead({
      owner,
      repo,
      basehead: `${previousTag}...${targetSha}`,
    });
    commits = data.commits;
  }

  const releaseHighlights = [];
  for (const pull_number of extractPrNumbers(commits)) {
    const { data: pr } = await github.rest.pulls.get({ owner, repo, pull_number });
    const skip = pr.labels.some((label) => label.name === "skip-release-notes");
    if (!skip) {
      categories.get(bucketFor(pr)).push(pr);
      releaseHighlights.push(...extractReleaseHighlights(pr.body));
    }
  }

  const totalPrs = [...categories.values()].reduce((sum, prs) => sum + prs.length, 0);
  const summaryParts = [...categories.entries()]
    .filter(([, prs]) => prs.length > 0)
    .map(([title, prs]) => pluralize(prs.length, summaryNouns[title]));

  const lines = [
    "## Release Notes",
    "",
    totalPrs
      ? `This release includes ${summaryParts.join(", ")} across ${pluralize(totalPrs, "merged PR")}.`
      : "This release packages the latest `rwd` changes with the project's structured release-note format.",
    "",
  ];

  const featurePrs = categories.get("New Features") ?? [];
  if (featurePrs.length) {
    lines.push("### New Features");
    for (const pr of featurePrs.slice(0, 3)) {
      lines.push(`- ${cleanTitle(pr.title)} (#${pr.number})`);
    }
    if (featurePrs.length > 3) {
      lines.push(`- Plus ${pluralize(featurePrs.length - 3, "more PR")} in this category.`);
    }
    lines.push("");
  }

  lines.push("### Highlights");
  lines.push(...releaseHighlights);
  if (previousTag) {
    lines.push(`- Compared against \`${previousTag}\` to keep the summary scoped to this release.`);
  }
  lines.push(`- Tagged as \`${tagName}\` from commit \`${targetSha.slice(0, 7)}\`.`);
  lines.push("- Install with `cargo install --git https://github.com/gigagookbob/rewind.git` or update with `rwd update`.");
  lines.push("");

  for (const [title, prs] of categories) {
    if (title === "New Features" || !prs.length) continue;

    lines.push(`### ${title}`);
    for (const pr of prs.slice(0, 3)) {
      lines.push(`- ${cleanTitle(pr.title)} (#${pr.number})`);
    }
    if (prs.length > 3) {
      lines.push(`- Plus ${pluralize(prs.length - 3, "more PR")} in this category.`);
    }
    lines.push("");
  }

  fs.writeFileSync(outputPath, lines.join("\n"));
  core.notice(`Wrote structured release intro to ${outputPath}`);
};

module.exports.extractReleaseHighlights = extractReleaseHighlights;
