# GitHub Actions

communiqué integrates naturally into CI/CD pipelines. This guide covers the most common GitHub Actions patterns.

## Prerequisites

Add these secrets to your repository (Settings > Secrets and variables > Actions):

| Secret | Required | Purpose |
|--------|----------|---------|
| `ANTHROPIC_API_KEY` | Yes | LLM access for generating release notes |
| `GITHUB_TOKEN` | Automatic | PR and release access (provided by Actions) |

## Basic: Update release on tag push

The simplest setup runs communiqué whenever you push a version tag:

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
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install communique
      - name: Generate release notes
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: communique generate "${{ github.ref_name }}" --github-release
```

`fetch-depth: 0` is required so communiqué can read the full git history between tags.

## With release-plz

[release-plz](https://release-plz.ieni.dev/) automates versioning, changelogs, and crate publishing. communiqué can enhance the release notes it creates.

```yaml
name: Release-plz

on:
  push:
    branches: [main]

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run release-plz
        id: release-plz
        uses: release-plz/action@main
        with:
          command: release
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
      - name: Enhance release notes
        if: steps.release-plz.outputs.releases_created == 'true'
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          cargo install communique
          for tag in $(echo '${{ steps.release-plz.outputs.releases }}' | jq -r '.[].tag'); do
            communique generate "$tag" --github-release
          done
```

This replaces the auto-generated release-plz notes with editorialized ones while keeping the automated publish workflow.

## Updating release PR descriptions

You can also use communiqué to preview release notes in the PR itself, so reviewers see what the release will look like before merging:

```yaml
  release-pr:
    runs-on: ubuntu-latest
    permissions:
      contents: write
      pull-requests: write
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Run release-plz
        id: release-plz
        uses: release-plz/action@main
        with:
          command: release-pr
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      - name: Update PR with release notes
        if: steps.release-plz.outputs.prs_created == 'true'
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          cargo install communique
          PR_NUMBER=$(echo '${{ steps.release-plz.outputs.pr }}' | jq -r '.number')
          gh pr checkout "$PR_NUMBER"

          VERSION=$(cargo metadata --format-version=1 --no-deps | jq -r '.packages[0].version')
          TAG="v${VERSION}"

          NOTES=$(communique generate "$TAG")
          PR_TITLE="$TAG: $(echo "$NOTES" | head -1 | sed 's/^# //')"
          PR_BODY=$(echo "$NOTES" | tail -n +3)
          gh pr edit "$PR_NUMBER" --title "$PR_TITLE" --body "$PR_BODY"
```

## Using `--concise` for changelogs

The `--concise` flag outputs just the changelog portion (no release title or narrative). This is useful for updating `CHANGELOG.md` in a PR:

```yaml
      - name: Update CHANGELOG.md
        run: |
          CONCISE=$(communique generate "$TAG" --concise)
          # Replace the release-plz generated entry with editorialized notes
          # (your replacement logic here)
```

## Dry run in PRs

Run communiqué in `--dry-run` mode on pull requests to preview release notes without publishing:

```yaml
name: Preview Release Notes

on:
  pull_request:
    branches: [main]

jobs:
  preview:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo install communique
      - name: Preview release notes
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
        run: |
          TAG="v$(cargo metadata --format-version=1 --no-deps | jq -r '.packages[0].version')"
          communique generate "$TAG" --dry-run
```

## Environment variables

| Variable | Description |
|----------|-------------|
| `ANTHROPIC_API_KEY` | API key for Anthropic (Claude models) |
| `OPENAI_API_KEY` | API key for OpenAI-compatible providers |
| `GITHUB_TOKEN` | Token for GitHub API access (PR details, releases) |

The provider is auto-detected from the model name: `claude-*` models use Anthropic, everything else uses OpenAI-compatible endpoints.
