# Contributing

Thanks for helping improve Collette.

## Development

Use the standard Rust toolchain configured by `rust-toolchain.toml`.

Before opening a pull request, run:

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

The CI also runs Markdown, YAML, and security audit checks.

## Coverage

The coverage workflow uploads Rust coverage reports to Codecov. Provision a
`CODECOV_TOKEN` GitHub Actions repository secret with the repository upload token
from Codecov. For this public repository, pull requests from forks can upload
coverage without access to that secret if tokenless uploads are enabled in
Codecov.

## Pull Requests

Pull requests should be small enough to review comfortably and should describe
the behavior change, the motivation, and any compatibility impact.

Use a changelog-ready pull request title. The title is included in generated
release notes, so prefer a short human sentence over an implementation detail.

Good examples:

- `✨ Add prefix range scans`
- `🐛 Fix cursor bounds on reverse scans`
- `📝 Document collection contracts`

Avoid vague titles:

- `Fix stuff`
- `Update code`
- `WIP`

## Emoji Taxonomy

Pull request titles may start with an emoji from this taxonomy. Emojis are not
mandatory, and this list is intentionally not rigid or exhaustive.

Emojis are kept in the generated changelog and act as lightweight visual
categories.

| Emoji | Category | Use for |
| --- | --- | --- |
| `✨` | API | API additions or improvements |
| `🐛` | Fix | Bug fixes |
| `🛠️` | Internal Logic | Internal behavior changes, whether they ship as a fix or a feature |
| `💾` | Storage Backend | MultiStore backend work |
| `⚡` | Performance | Performance improvements |
| `🧪` | Tests | Test-only changes |
| `📝` | Docs | Documentation-only changes |
| `♻️` | Refactor | Internal changes with no behavior change |
| `🏗️` | CI/Build | CI, build, packaging, and release automation |
| `⬆️` | Dependencies | Dependency updates |
| `🔒` | Security | Security fixes or hardening |

If multiple categories apply, choose the one that best describes the user-facing
impact. For example, a bug fix with tests should use `🐛`, not `🧪`.

## Release Intent

Release versioning is computed from Conventional Commits. Labels do not request
or override version bumps.

When preparing a pull request, check that the commits match the intended release
impact:

- `fix:` and `perf:` trigger patch releases.
- `feat:` triggers minor releases.
- `!` markers and `BREAKING CHANGE:` footers mark breaking releases.
  They also apply the `breaking-change` label.
- `docs:`, `test:`, `refactor:`, `ci:`, and `chore:` do not trigger releases
  by themselves.

Pull requests that intentionally break the public API must include a
Conventional Commit breaking marker in at least one commit. Use `!` in the
commit header or add a `BREAKING CHANGE:` footer. Without that marker, the
breaking-change detection workflow fails when it detects a breaking public API
change. When that happens, the workflow comments on the pull request with the
detected public API changes.

## Pull Request Labels

Pull request labels are computed by CI from commits and changed files. Before
merging, check that the labels match the intent of the pull request.

Labels help the changelog land in the right section. They do not request or
override a version bump; fix the commits instead.

Examples of labels include:

- `enhancement` for feature work
- `bug` for fixes
- `security` for security hardening or fixes
- `documentation` for documentation-only changes
- `dependencies` for dependency updates
- `ci` for CI, build, and release automation
- `breaking-change` for breaking API changes

This list is not exhaustive. Prefer the label set that best reflects the PR.

## Commits

Commit messages must follow Conventional Commits:

Examples:

```text
feat: add prefix range scans
fix: handle empty cursor bounds
feat!: rename collection builder API
```

For breaking changes, include a footer explaining the migration:

```text
BREAKING CHANGE: collection builders now require an explicit backend type.
```

## Releases

Releases are cut manually from `main`. See `RELEASE.md` for the release process.
