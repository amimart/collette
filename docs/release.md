# Release Process

Colette releases are cut manually from the `main` branch.

## Versioning

The release workflow computes the next version from Conventional Commits since
the latest `v*` tag:

- `fix:` and `perf:` bump the patch version.
- `feat:` bumps the minor version.
- `!` markers and `BREAKING CHANGE:` footers mark a breaking release.

While the crate is on `0.x`, breaking releases bump the minor version by
default. Set `RELEASE_PRE_1_0_BREAKING_AS=major` in the workflow environment if
that policy changes.

## Changelog

The workflow asks GitHub to generate release notes from merged pull requests,
cleans the generated body, and prepends the result to `CHANGELOG.md`.

The same cleaned notes are used as the GitHub Release body.

Leading emoji in pull request titles are removed from the generated changelog.
For example, `🛠️ Fix cursor based scan` becomes `Fix cursor based scan`.

Pull requests can be grouped with these labels:

- `release:breaking`
- `release:feature`
- `release:fix`
- `release:docs`
- `dependencies`
- `release:skip` or `skip-changelog`

## Cutting a Release

1. Run the `Release` workflow manually.
2. Keep `dry_run` enabled first to validate the computed release.
3. Run it again with `dry_run` disabled.

The workflow commits `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md` back to
`main`, creates a `vX.Y.Z` tag, pushes the tag, and creates the GitHub Release.

Publishing to crates.io is intentionally handled by a separate workflow.
