# communique

AI-generated release notes for people who read release notes.

communique turns your git history, pull requests, and repository context into
polished release notes. It can print notes locally, update `CHANGELOG.md`, or
publish directly to a GitHub Release.

## Install

Install with [mise](https://mise.jdx.dev):

```sh
mise use communique
```

Or install with Cargo:

```sh
cargo install communique
```

Pre-built binaries for macOS, Linux, and Windows are available on the
[GitHub releases page](https://github.com/jdx/communique/releases).

## Setup

communique needs an LLM API key. Claude models use Anthropic; other models use
the OpenAI-compatible provider path.

```sh
# Default Claude models
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenAI-compatible models
export OPENAI_API_KEY="sk-..."
```

GitHub context and GitHub Release publishing also need a token:

```sh
export GITHUB_TOKEN="$(gh auth token)"
```

Initialize a config file in your repository:

```sh
communique init
```

This creates `communique.toml`, where you can set the default model, repository,
style instructions, and project context.

## Generate Release Notes

Generate notes for a tag:

```sh
communique generate v1.2.0
```

communique automatically detects the previous tag. You can provide it explicitly
when needed:

```sh
communique generate v1.2.0 v1.1.0
```

Preview without publishing or verifying links:

```sh
communique generate v1.2.0 --dry-run
```

Write the output to a file:

```sh
communique generate v1.2.0 --output RELEASE_NOTES.md
```

## Update GitHub Releases

Publish generated notes to an existing GitHub Release:

```sh
communique generate v1.2.0 --github-release
```

communique reads commits and pull requests, lets the model inspect relevant
files and diffs, verifies links, then replaces the GitHub Release body with the
finished notes.

## Update CHANGELOG.md

Add or replace the version entry in `CHANGELOG.md`:

```sh
communique generate v1.2.0 --changelog
```

For release PRs, this keeps the `[Unreleased]` section in place and inserts the
new version entry immediately below it.

Use concise changelog output when you do not want a full release narrative:

```sh
communique generate v1.2.0 --changelog --concise
```

## Configuration

`communique.toml` supports project-level defaults and writing guidance:

```toml
context = """
This is a Rust CLI used by platform engineers.
Call out breaking changes and migration steps clearly.
"""

system_extra = """
Write in a direct, practical tone.
Avoid marketing language.
"""

[defaults]
model = "claude-opus-4-7"
repo = "owner/repo"
max_tokens = 4096
```

CLI flags override config defaults.

## GitHub Actions

Use `fetch-depth: 0` so communique can compare tags:

```yaml
name: Release Notes

on:
  push:
    tags:
      - v[0-9]+.*

permissions:
  contents: write

jobs:
  release-notes:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - run: cargo install communique
      - name: Generate release notes
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: communique generate "${{ github.ref_name }}" --github-release
```

See the [GitHub Actions guide](docs/guide/github-actions.md) for release-plz
examples and release PR workflows.

## Common Options

```sh
communique generate v1.2.0 --repo owner/repo
communique generate v1.2.0 --model claude-opus-4-7
communique generate v1.2.0 --provider openai --base-url https://api.example.com/v1
communique generate v1.2.0 --quiet
communique generate v1.2.0 --verbose
```

## Troubleshooting

If PR details are missing, confirm `GITHUB_TOKEN` is set and can read the
repository.

If generation fails because tags cannot be compared, fetch full history and
tags:

```sh
git fetch --tags --unshallow
```

If `CHANGELOG.md` updates fail, make sure the file has an `## [Unreleased]` or
`## Unreleased` section.

## Documentation

- [Getting started](docs/guide/getting-started.md)
- [Configuration](docs/guide/configuration.md)
- [GitHub Actions](docs/guide/github-actions.md)
- [CLI reference](docs/cli/index.md)
