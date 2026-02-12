# communiqué

Editorialized release notes powered by AI.

communiqué is a CLI tool that uses an AI agent to generate professional, narrative release notes from your git history, PRs, and source code. Currently powered by Claude.

## Install

```sh
cargo install communique
```

## Quick Start

```sh
export ANTHROPIC_API_KEY="sk-ant-..."
export GITHUB_TOKEN="ghp_..."       # optional, for PR context and publishing

communique init                      # create communique.toml
communique generate v1.0.0           # generate release notes
communique generate v1.0.0 --github-release  # generate and publish
```

## Roadmap

### Features

- [ ] Multi-provider LLM support (abstract behind a trait, add OpenAI/etc.)
- [ ] Dry-run mode — preview before publishing to GitHub
- [ ] Progress indication (spinner/status while fetching PRs and waiting on LLM)
- [ ] Output to file (`--output <path>`)
- [ ] `--verbose`/`--quiet` flags instead of requiring `RUST_LOG`
- [ ] Non-GitHub forge support (GitLab, Gitea)
- [ ] Parallel PR fetching (tokio is already a dependency)
- [ ] Retry with exponential backoff on API calls (Anthropic and GitHub)
- [ ] Validate git tag exists before proceeding
- [ ] Batch mode — generate notes for multiple tags in one run

### Code Quality

- [ ] Replace `.unwrap()`/`.expect()` with proper error returns (`anthropic.rs`, `git.rs`, `main.rs`)
- [ ] Make output parsing more resilient — fallback if `---SECTION_BREAK---` is missing
- [ ] Use `LazyLock` for compiled regex in `git.rs:extract_pr_numbers()`
- [ ] Avoid cloning full message history every agent loop iteration
- [ ] Update `anthropic-version` header (currently pinned to `2023-06-01`)
- [ ] Config validation (e.g. `max_tokens > 0`)

### Testing

- [ ] Unit tests for output parsing (`output::parse()`)
- [ ] Unit tests for config loading and validation
- [ ] Unit tests for all tools (`read_file`, `list_files`, `grep`, `get_pr`, `get_pr_diff`)
- [ ] Unit tests for changelog entry reading
- [ ] Integration tests with mocked API responses
- [ ] CI test workflow
