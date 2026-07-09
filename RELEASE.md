# Release Process

Colette releases are cut manually from the `main` branch.

## Versioning

The crate version in `Cargo.toml` is expected to be up to date before the
release workflow runs. The workflow checks that version against the next version
computed from Conventional Commits since the latest `v*` tag:

- `fix:` and `perf:` bump the patch version.
- `feat:` bumps the minor version.
- `!` markers and `BREAKING CHANGE:` footers mark a breaking release.

For the first release, when no previous `v*` tag exists, the workflow releases
the current `Cargo.toml` version.

If there are no releasable commits since the latest tag, the workflow fails
without creating a release.

While the crate is on `0.x`, breaking changes are expected to bump the minor
version by default. Change `RELEASE_PRE_1_0_BREAKING_AS` in the release workflow
to `major` if that policy changes.

## Changelog

The workflow asks GitHub to generate release notes from merged pull requests,
cleans the generated body, and prepends the result to `CHANGELOG.md`.

The same cleaned notes are used as the GitHub Release body.

Pull request title emojis are preserved in the generated changelog and can be
used as lightweight visual categories.

Release note sections are grouped with ordinary pull request labels:

- `breaking-change`
- `enhancement`
- `bug`
- `security`
- `documentation`
- `dependencies`
- `ci`

These labels do not control the released version. The version is computed from
Conventional Commits.

## Cutting a Release

1. Run the `Release` workflow manually.
2. The workflow runs the existing build, test, lint, and audit workflows.
3. Keep `dry_run` enabled first to validate the computed release.
4. Run it again with `dry_run` disabled.

The workflow commits `CHANGELOG.md` back to `main`, creates a `vX.Y.Z` tag,
pushes the tag, and creates the GitHub Release.

Publishing to crates.io is intentionally handled by a separate workflow.
