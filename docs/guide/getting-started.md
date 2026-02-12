# Getting Started

## Installation

Install with [mise](https://mise.jdx.dev) (recommended):

```sh
mise use communique
```

Or with cargo:

```sh
cargo install communique
```

Pre-built binaries for macOS, Linux, and Windows are also available on the [GitHub releases page](https://github.com/jdx/communique/releases).

## Prerequisites

communiqué needs an API key for the LLM provider that will generate your release notes. Set one of the following:

```sh
# For Claude models (default)
export ANTHROPIC_API_KEY="sk-ant-..."

# For OpenAI-compatible providers
export OPENAI_API_KEY="sk-..."
```

The provider is auto-detected from the model name: `claude-*` models use Anthropic, everything else uses OpenAI-compatible endpoints.

For GitHub features (reading PR details, publishing releases), you also need a GitHub token:

```sh
export GITHUB_TOKEN="ghp_..."
```

The token needs the `repo` scope (or `public_repo` for public repositories). The easiest way to create one:

```sh
gh auth token  # if you use the GitHub CLI
```

Or create a [personal access token](https://github.com/settings/tokens) in GitHub settings.

## Quick Start

### 1. Initialize your config

Generate a `communique.toml` in your repository root:

```sh
communique init
```

### 2. Generate release notes

Generate release notes for a specific git tag:

```sh
communique generate v1.0.0
```

communiqué automatically finds the previous tag, gathers the git history and PR data, and produces editorialized release notes.

### 3. Publish to GitHub

Write the release notes directly to a GitHub Release:

```sh
communique generate v1.0.0 --github-release
```

## How It Works

1. Scans your git history between two tags
2. Extracts PR references from commit messages
3. Fetches PR details and diffs from GitHub (if token provided)
4. Sends context to an LLM equipped with codebase exploration tools
5. The agent reads files, searches code, and builds a mental model of the changes
6. Outputs a concise changelog entry and a detailed release narrative
