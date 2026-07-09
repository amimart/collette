#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const changelogPath = "CHANGELOG.md";
const cargoTomlPath = "Cargo.toml";
const cargoLockPath = "Cargo.lock";
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

function packageName() {
  const manifest = readFileSync(cargoTomlPath, "utf8");
  const name = manifest.match(/^name = "([^"]+)"$/m)?.[1];

  if (!name) {
    throw new Error("Could not read package name from Cargo.toml");
  }

  return name;
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

function cleanReleaseNotes(notes) {
  return notes
    .replace(/^## What's Changed\s*/m, "")
    .replace(/\n## New Contributors[\s\S]*?(?=\n\*\*Full Changelog\*\*:|\n## |\s*$)/m, "")
    .split("\n")
    .map((line) =>
      line.replace(
        /^(\s*[-*]\s+)(?:[\p{Extended_Pictographic}\uFE0F\u200D]+\s*)+/u,
        "$1",
      ),
    )
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

function updateCargoToml(version) {
  const manifest = readFileSync(cargoTomlPath, "utf8");
  const updated = manifest.replace(/^version = "[^"]+"$/m, `version = "${version}"`);

  if (manifest === updated) {
    throw new Error("Cargo.toml version was not updated");
  }

  writeFileSync(cargoTomlPath, updated);
}

function updateCargoLock(version) {
  if (!existsSync(cargoLockPath)) {
    return;
  }

  const name = packageName();
  const lockfile = readFileSync(cargoLockPath, "utf8");
  const packageBlock = new RegExp(
    `(\\[\\[package\\]\\]\\nname = "${name}"\\nversion = ")[^"]+(")`,
  );
  const updated = lockfile.replace(packageBlock, `$1${version}$2`);

  if (lockfile === updated) {
    throw new Error(`Could not update ${name} package version in ${cargoLockPath}`);
  }

  writeFileSync(cargoLockPath, updated);
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
  const version = currentVersion();
  const bump = strongestBump(parseCommits(previousTag));

  if (!bump) {
    writeOutput({ release_required: "false" });
    return;
  }

  const nextVersion = incrementVersion(version, bump);

  writeOutput({
    release_required: "true",
    previous_tag: previousTag,
    version: nextVersion,
    tag: `v${nextVersion}`,
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

  updateCargoToml(version);
  updateCargoLock(version);
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
