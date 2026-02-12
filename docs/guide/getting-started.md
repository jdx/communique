# Getting Started

Welcome, operator. Follow these steps to initialize your communiqué installation.

## Installation

Acquire the binary from the central registry:

```sh
cargo install communique
```

## Prerequisites

You will need to supply valid credentials for neural network access:

```sh
export ANTHROPIC_API_KEY="sk-ant-..."
```

For GitHub uplink capabilities (PR reconnaissance, release publishing), also provide:

```sh
export GITHUB_TOKEN="ghp_..."
```

## Quick Start

### 1. Initialize your workspace

Generate a `communique.toml` manifest in your repository root:

```sh
communique init
```

### 2. Generate a communiqué

Synthesize release notes for a specific git tag:

```sh
communique generate v1.0.0
```

The system will automatically locate the previous tag, gather all relevant history and PR data, and produce editorialized release notes.

### 3. Publish to GitHub

Transmit the finished communiqué directly to a GitHub Release:

```sh
communique generate v1.0.0 --github-release
```

## How It Works

1. Scans your git history between two tags
2. Extracts PR references from commit messages
3. Fetches PR details and diffs from the GitHub uplink (if token provided)
4. Dispatches context to an LLM equipped with codebase exploration tools
5. The agent reads files, searches code, and builds a mental model of the changes
6. Outputs a concise changelog entry and a detailed release narrative
