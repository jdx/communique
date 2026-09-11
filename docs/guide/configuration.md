# Configuration

communiqué is configured via a `communique.toml` manifest in your repository root. Generate one with:

```sh
communique init
```

## Directives

### `system_extra`

Additional instructions injected into the system prompt. Use this to shape the tone, style, or conventions of your output.

```toml
system_extra = """
Write in a casual, friendly tone.
Always mention breaking changes prominently.
"""
```

### `context`

Supplemental context included in every generation request. Useful for project descriptions or domain knowledge the agent should always have access to.

```toml
context = """
This is a Rust CLI tool for managing cloud infrastructure.
Our users are DevOps engineers and SREs.
"""
```

### `[defaults]`

Default parameters for generation. All values can be overridden via CLI flags.

```toml
[defaults]
model = "claude-fable-5-1"
max_tokens = 16384
repo = "owner/repo"
```

| Key | Description | Default |
|-----|-------------|---------|
| `model` | Model identifier | `claude-fable-5-1` |
| `max_tokens` | Maximum tokens permitted per model response (billing is based on actual usage) | `16384` |
| `repo` | GitHub repo in `owner/repo` format | Auto-detected from git remote |

## Resolution Order

Configuration is resolved with the following precedence:

1. CLI flags (highest priority)
2. `communique.toml` defaults
3. Built-in defaults

## Package releases

Define separate scopes and changelog destinations in a monorepo:

```toml
[packages.cli]
paths = ["crates/cli", "crates/shared"]
tag_pattern = "cli/v*"
channel = "stable"
changelog_path = "crates/cli/CHANGELOG.md"
```

Run `communique generate cli/v2.0.0 --package cli --changelog`.
CLI `--path` values replace the configured paths; other explicit flags override
individual package defaults. Paths are literal repository-relative files or
directories, not glob patterns. The model can inspect other files for context,
but is instructed to describe only changes in the filtered commit list.

`--tag-pattern 'cli/v*'` filters candidate baseline tags using Git's glob syntax.
Candidates must be reachable from the target commit. `--channel stable` excludes
prerelease tags (such as `v2.0.0-beta.1`); `--channel all` includes them
and is the default. An explicit previous reference takes precedence over automatic
selection. The selected range is printed before generation.

## Editorial label rules

```toml
[rules]
include_labels = ["release-note:include"]
exclude_labels = ["release-note:skip", "internal"]

[rules.categories]
"bug" = "Fixed"
"enhancement" = "Added"
```

Rules use labels from PR references in commit subjects (`(#123)`). They require
`GITHUB_TOKEN`; inaccessible PRs cause generation to fail rather than silently
ignore the rules. Include labels override exclude labels and the default filter
for internal changes. Excluded commits are removed from the initial change list;
inclusion and category rules are supplied as explicit model instructions.
Commits without recognized PR references use the normal editorial policy.

## Review, edit, and publish

```sh
communique generate v2.0.0 --draft release.json --review-report review.md \
  --migration-guide upgrade.md
# Edit release_title, release_body, or changelog in release.json.
communique publish release.json --dry-run
communique publish release.json
```

The JSON draft records the repository, tag, target commit, previous reference,
title, body, changelog, and review. Publishing uses the edited title and body
without another model request. Before updating, it resolves the remote tag and
requires it to match the commit recorded when generation began. A moved or missing
tag stops publication; regenerate and review the draft before trying again.
The plain `--dry-run` preview is offline and does not check the remote tag.
Publishing updates an existing GitHub Release and requires
`GITHUB_TOKEN`; it does not write the saved changelog to disk. Draft generation
cannot be combined with `--github-release`.

Coverage reports list each scoped commit as included, omitted, or uncertain with
a source link and reason. Model assessments are advisory; missing, duplicate, or
invalid assessments become uncertain, and explicit label exclusions are recorded
separately. A requested migration guide contains affected users, upgrade steps,
and verified examples, or an explicit statement that no migration is needed.
If the model fails to return a requested guide, generation fails before publishing.

### Preserve hand-written release text

Use `--preserve-sections` with `--github-release` or `--draft` to manage only this
part of a release:

```markdown
Installation instructions and announcements stay here.

<!-- communique:start -->
Generated release notes go here.
<!-- communique:end -->

Maintainer notes stay here too.
```

The model receives only managed content as existing-release and style context,
and is instructed to omit markers and surrounding hand-written text.
If markers are absent, a marked section is appended to the existing body. Invalid
or repeated markers cause an error. The existing release title is preserved in
this mode. `publish --dry-run` fetches the existing release when merging a marked
section, so that preview requires GitHub authentication too.
