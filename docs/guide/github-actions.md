# GitHub Actions

communiqué integrates naturally into CI/CD pipelines. This guide covers the most common GitHub Actions patterns.

## Prerequisites

Add your LLM provider secret to your repository (Settings > Secrets and variables > Actions):

| Secret | Purpose |
|--------|---------|
| `ANTHROPIC_API_KEY` | API key for Claude models |
| `OPENAI_API_KEY` | API key for OpenAI-compatible providers |
| `GITHUB_TOKEN` | PR and release access (provided automatically by Actions) |

You only need one of `ANTHROPIC_API_KEY` or `OPENAI_API_KEY`, depending on which provider you use.

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
          # Use whichever provider you've configured:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
          # OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
        run: communique generate "${{ github.ref_name }}" --github-release
```

`fetch-depth: 0` is required so communiqué can read the full git history between tags.

## With release-plz

[release-plz](https://release-plz.ieni.dev/) automates versioning, changelogs, and crate publishing. communiqué can enhance the release notes it creates.

### Enhancing release notes

After release-plz creates a release, communiqué replaces the auto-generated notes with editorialized ones:

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
      - name: Fetch new tags
        if: steps.release-plz.outputs.releases_created == 'true'
        run: git fetch --tags
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

> **Note:** The `git fetch --tags` step is required because release-plz creates tags via the GitHub API, so they're not present in the local checkout.

### Updating release PRs and CHANGELOG.md

Use `--changelog` to update `CHANGELOG.md` with AI-generated notes and update the PR title/body for reviewers:

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

          # Generate release notes and update CHANGELOG.md in one pass
          NOTES=$(communique generate "$TAG" --changelog)

          # Update PR title and body
          PR_TITLE=$(echo "$NOTES" | head -1 | sed 's/^# //')
          PR_BODY=$(echo "$NOTES" | tail -n +3)
          gh pr edit "$PR_NUMBER" --title "$PR_TITLE" --body "$PR_BODY"

          # Commit and push changelog changes
          if ! git diff --quiet CHANGELOG.md 2>/dev/null; then
            git add CHANGELOG.md
            git commit -m "chore: update changelog with communique release notes"
            git push
          fi
```

The `--changelog` flag makes a single LLM call to intelligently insert or update the entry in `CHANGELOG.md`, matching the existing file's formatting conventions.

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
