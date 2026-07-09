# Contributing

Thanks for helping improve Colette.

## Development

Use the standard Rust toolchain configured by `rust-toolchain.toml`.

Before opening a pull request, run:

```sh
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

The CI also runs Markdown, YAML, and security audit checks.

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

## Release Labels

Release notes are grouped by pull request labels. Add one of these labels when
opening or merging a pull request:

- `release:breaking`
- `release:feature`
- `release:fix`
- `release:security`
- `release:skip` or `skip-changelog`

Use `release:skip` for changes that should not appear in the changelog.

Pull requests that intentionally break the public API must use
`release:breaking` or `breaking-change`. Without one of those labels, the
breaking-change detection workflow fails when it detects a breaking public API
change.

The `bug` and `enhancement` labels are applied automatically from Conventional
Commit signals in the pull request title or commits:

- `fix:` applies `bug`
- `feat:` applies `enhancement`

The `ci` and `documentation` labels are applied automatically from changed
files.

## Commits

Commit messages must follow Conventional Commits:

- `fix:` and `perf:` trigger patch releases.
- `feat:` triggers minor releases.
- `!` markers and `BREAKING CHANGE:` footers mark breaking releases.
- `docs:`, `test:`, `refactor:`, `ci:`, and `chore:` do not trigger releases
  by themselves.

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
