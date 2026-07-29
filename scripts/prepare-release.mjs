#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const changelogPath = "CHANGELOG.md";
const cargoTomlPath = "Cargo.toml";
const cleanNotesPath = "release-notes-clean.md";
const releaseNotesTemplatePath = ".github/release-notes.md.tpl";
const releaseSections = [
  ["breaking_changes", "Breaking Changes"],
  ["features", "Features"],
  ["bug_fixes", "Bug Fixes"],
  ["security", "Security"],
  ["other_changes", "Other Changes"],
];
const releaseLabelSections = new Map([
  ["breaking-change", "breaking_changes"],
  ["enhancement", "features"],
  ["bug", "bug_fixes"],
  ["security", "security"],
]);

function run(command, args, options = {}) {
  return execFileSync(command, args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    ...options,
  }).trim();
}

function tryRun(command, args) {
  try {
    return run(command, args);
  } catch {
    return "";
  }
}

function writeOutput(values) {
  const outputPath = process.env.GITHUB_OUTPUT;

  for (const [name, value] of Object.entries(values)) {
    console.log(`${name}=${value}`);
  }

  if (!outputPath) {
    return;
  }

  const lines = Object.entries(values).map(([name, value]) => `${name}=${value}`);
  writeFileSync(outputPath, `${lines.join("\n")}\n`, { flag: "a" });
}

function currentVersion() {
  const manifest = readFileSync(cargoTomlPath, "utf8");
  const version = manifest.match(/^version = "([^"]+)"$/m)?.[1];

  if (!version) {
    throw new Error("Could not read package version from Cargo.toml");
  }

  return version;
}

function updateCargoTomlVersion(version) {
  const manifest = readFileSync(cargoTomlPath, "utf8");
  const updated = manifest.replace(
    /^version = "[^"]+"$/m,
    `version = "${version}"`,
  );

  if (updated === manifest) {
    throw new Error("Could not update package version in Cargo.toml");
  }

  writeFileSync(cargoTomlPath, updated);
}

function updateCargoLockVersion(version) {
  const cargoLockPath = "Cargo.lock";

  if (!existsSync(cargoLockPath)) {
    return;
  }

  const lockfile = readFileSync(cargoLockPath, "utf8");
  const packagePattern = /(\[\[package\]\]\nname = "collette"\nversion = ")[^"]+(")/;
  const updated = lockfile.replace(packagePattern, `$1${version}$2`);

  if (updated === lockfile) {
    throw new Error("Could not update collette package version in Cargo.lock");
  }

  writeFileSync(cargoLockPath, updated);
}

function latestTag() {
  return tryRun("git", ["describe", "--tags", "--match", "v[0-9]*", "--abbrev=0"]);
}

function parseCommits(previousTag) {
  const range = previousTag ? `${previousTag}..HEAD` : "HEAD";
  const output = tryRun("git", [
    "log",
    "--format=%H%x00%s%x00%b%x1e",
    range,
  ]);

  return output
    .split("\x1e")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [hash, subject, body = ""] = entry.split("\x00");
      return { hash, subject, body };
    });
}

function parseFirstParentCommits(previousTag) {
  const range = previousTag ? `${previousTag}..HEAD` : "HEAD";
  const output = tryRun("git", [
    "log",
    "--first-parent",
    "--reverse",
    "--format=%H%x00%s%x00%b%x00%P%x1e",
    range,
  ]);

  return output
    .split("\x1e")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [hash, subject, body = "", parents = ""] = entry.split("\x00");
      return { hash, subject, body, parents: parents.split(" ").filter(Boolean) };
    });
}

function commitBump(commit) {
  const subject = commit.subject.trim();
  const body = commit.body.trim();
  const conventional = subject.match(/^([a-z]+)(?:\([^)]+\))?(!)?:\s+.+$/);

  if (conventional?.[2] || /\bBREAKING[ -]CHANGE:/m.test(body)) {
    return "major";
  }

  if (!conventional) {
    return null;
  }

  if (conventional[1] === "feat") {
    return "minor";
  }

  if (["fix", "perf"].includes(conventional[1])) {
    return "patch";
  }

  return null;
}

function strongestBump(commits) {
  const priority = { patch: 1, minor: 2, major: 3 };
  let selected = null;

  for (const commit of commits) {
    const bump = commitBump(commit);

    if (bump && (!selected || priority[bump] > priority[selected])) {
      selected = bump;
    }
  }

  return selected;
}

function incrementVersion(version, bump) {
  const match = version.match(/^(\d+)\.(\d+)\.(\d+)(?:-.+)?$/);

  if (!match) {
    throw new Error(`Unsupported Cargo.toml version: ${version}`);
  }

  let [, major, minor, patch] = match.map(Number);

  if (bump === "major") {
    if (major === 0 && process.env.RELEASE_PRE_1_0_BREAKING_AS !== "major") {
      minor += 1;
      patch = 0;
    } else {
      major += 1;
      minor = 0;
      patch = 0;
    }
  } else if (bump === "minor") {
    minor += 1;
    patch = 0;
  } else if (bump === "patch") {
    patch += 1;
  } else {
    throw new Error(`Unsupported bump: ${bump}`);
  }

  return `${major}.${minor}.${patch}`;
}

function tagVersion(tag) {
  const version = tag.replace(/^v/, "");

  if (!/^\d+\.\d+\.\d+(?:-.+)?$/.test(version)) {
    throw new Error(`Unsupported release tag: ${tag}`);
  }

  return version;
}

function repositoryUrl() {
  if (process.env.GITHUB_REPOSITORY) {
    return `https://github.com/${process.env.GITHUB_REPOSITORY}`;
  }

  const manifest = readFileSync(cargoTomlPath, "utf8");
  return manifest.match(/^repository = "([^"]+)"$/m)?.[1] ?? "";
}

function pullRequestInfo(commit) {
  const match = commit.subject.match(/^Merge pull request #(\d+) from .+$/);

  if (!match) {
    return null;
  }

  const title = commit.body
    .split("\n")
    .map((line) => line.trim())
    .find(Boolean);

  return {
    number: match[1],
    title: title || commit.subject,
  };
}

function mockedPullRequestLabels() {
  const raw = process.env.RELEASE_NOTES_PR_LABELS;

  if (!raw) {
    return null;
  }

  return new Map(
    Object.entries(JSON.parse(raw)).map(([number, labels]) => [
      number,
      labels.map((label) => label.toLowerCase()),
    ]),
  );
}

function pullRequestLabels(number, mockedLabels = mockedPullRequestLabels()) {
  if (mockedLabels?.has(number)) {
    return mockedLabels.get(number);
  }

  const output = run("gh", [
    "pr",
    "view",
    number,
    "--json",
    "labels",
    "--jq",
    ".labels[].name",
  ]);

  return output
    .split("\n")
    .map((label) => label.trim().toLowerCase())
    .filter(Boolean);
}

function sectionKeysForLabels(labels) {
  const keys = new Set();

  for (const label of labels) {
    const key = releaseLabelSections.get(label);

    if (key) {
      keys.add(key);
    }
  }

  if (keys.size === 0) {
    keys.add("other_changes");
  }

  return keys;
}

function releaseItem(commit, repoUrl) {
  const pr = pullRequestInfo(commit);

  if (pr && repoUrl) {
    return `* ${pr.title} ([#${pr.number}](${repoUrl}/pull/${pr.number}))`;
  }

  if (pr) {
    return `* ${pr.title} (#${pr.number})`;
  }

  const shortHash = commit.hash.slice(0, 7);
  return repoUrl
    ? `* ${commit.subject} ([${shortHash}](${repoUrl}/commit/${commit.hash}))`
    : `* ${commit.subject} (${shortHash})`;
}

function releaseEntries(previousTag) {
  const repoUrl = repositoryUrl();
  const sections = Object.fromEntries(releaseSections.map(([key]) => [key, []]));
  const mockedLabels = mockedPullRequestLabels();

  for (const commit of parseFirstParentCommits(previousTag)) {
    if (/^chore\(release\): v\d+\.\d+\.\d+/.test(commit.subject)) {
      continue;
    }

    const item = releaseItem(commit, repoUrl);
    const pr = pullRequestInfo(commit);
    const labels = pr ? pullRequestLabels(pr.number, mockedLabels) : [];
    const sectionKeys = pr ? sectionKeysForLabels(labels) : new Set(["other_changes"]);

    for (const key of sectionKeys) {
      sections[key].push(item);
    }
  }

  return sections;
}

function fullChangelogUrl(previousTag, tag) {
  const repoUrl = repositoryUrl();

  if (!repoUrl) {
    return "";
  }

  return previousTag
    ? `${repoUrl}/compare/${previousTag}...${tag}`
    : `${repoUrl}/commits/${tag}`;
}

function renderReleaseNotes(previousTag, tag) {
  const sections = releaseEntries(previousTag);
  const replacements = {};

  for (const [key, title] of releaseSections) {
    replacements[key] = sections[key].length
      ? `### ${title}\n\n${sections[key].join("\n")}\n`
      : "";
  }

  const changelogUrl = fullChangelogUrl(previousTag, tag);
  replacements.full_changelog = changelogUrl
    ? `**Full Changelog**: <${changelogUrl}>`
    : "";

  return readFileSync(releaseNotesTemplatePath, "utf8")
    .replace(/{{([a-z_]+)}}/g, (_, key) => replacements[key] ?? "")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function updateChangelog(version, notes) {
  const date = new Date().toISOString().slice(0, 10);
  const heading = `## v${version} - ${date}`;
  const entry = `${heading}\n\n${notes}\n`;
  const current = existsSync(changelogPath)
    ? readFileSync(changelogPath, "utf8").trimEnd()
    : "# Changelog";

  if (current.includes(heading)) {
    throw new Error(`${heading} already exists in ${changelogPath}`);
  }

  const updated = current.startsWith("# Changelog")
    ? current.replace("# Changelog", `# Changelog\n\n${entry}`)
    : `# Changelog\n\n${entry}\n\n${current}`;

  writeFileSync(changelogPath, `${updated.trimEnd()}\n`);
}

function plan() {
  const previousTag = latestTag();
  const current = currentVersion();
  const bump = strongestBump(parseCommits(previousTag));

  if (!previousTag) {
    writeOutput({
      previous_tag: "",
      version: current,
      tag: `v${current}`,
      bump: bump ?? "initial",
    });
    return;
  }

  if (!bump) {
    throw new Error("No feat, fix, perf, or breaking changes found since the latest release tag");
  }

  const expectedVersion = incrementVersion(tagVersion(previousTag), bump);

  writeOutput({
    previous_tag: previousTag,
    version: expectedVersion,
    tag: `v${expectedVersion}`,
    bump,
  });
}

function apply(notesFile) {
  const version = process.env.RELEASE_VERSION;

  if (!version) {
    throw new Error("RELEASE_VERSION is required");
  }

  if (!notesFile) {
    throw new Error("Usage: prepare-release.mjs apply <notes-file>");
  }

  const notes = readFileSync(notesFile, "utf8").trim();

  updateCargoTomlVersion(version);
  updateCargoLockVersion(version);
  updateChangelog(version, notes);
  writeFileSync(cleanNotesPath, `${notes}\n`);
}

function notes() {
  const previousTag = process.env.PREVIOUS_TAG || latestTag();
  const tag = process.env.TAG || `v${currentVersion()}`;
  const rendered = renderReleaseNotes(previousTag, tag);

  if (!rendered) {
    throw new Error("Release notes are empty");
  }

  console.log(rendered);
}

const [command, notesFile] = process.argv.slice(2);

if (command === "plan") {
  plan();
} else if (command === "notes") {
  notes();
} else if (command === "apply") {
  apply(notesFile);
} else {
  throw new Error("Usage: prepare-release.mjs <plan|notes|apply> [notes-file]");
}
