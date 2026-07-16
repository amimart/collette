#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const changelogPath = "CHANGELOG.md";
const cargoTomlPath = "Cargo.toml";
const cleanNotesPath = "release-notes-clean.md";

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

function cleanReleaseNotes(notes) {
  return notes
    .replace(/<!-- Release notes generated using configuration in \.github\/release\.yml at .+? -->\s*/g, "")
    .replace(/^## What's Changed\s*/m, "")
    .replace(/(?:^|\n)## New Contributors\s+[\s\S]*?(?=\n\*\*Full Changelog\*\*:|\n## |\s*$)/m, "\n")
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

  const notes = cleanReleaseNotes(readFileSync(notesFile, "utf8"));

  updateCargoTomlVersion(version);
  updateCargoLockVersion(version);
  updateChangelog(version, notes);
  writeFileSync(cleanNotesPath, `${notes}\n`);
}

const [command, notesFile] = process.argv.slice(2);

if (command === "plan") {
  plan();
} else if (command === "apply") {
  apply(notesFile);
} else {
  throw new Error("Usage: prepare-release.mjs <plan|apply> [notes-file]");
}
