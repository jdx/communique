# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Communiqué is a Rust CLI tool that generates AI-powered editorialized release notes. It uses an agentic loop where an LLM (Claude or OpenAI-compatible) reads repository context via tools (read_file, list_files, grep, get_pr, get_pr_diff) and produces structured release notes via a submit_release_notes tool.

## Commands

- `cargo build` — build the project
- `cargo test` — run all tests
- `cargo test <test_name>` — run a single test
- `cargo clippy` — lint
- `cargo fmt` — format
- `mise run lint` — run all linters (cargo-clippy + cargo-fmt via hk)

## Architecture

**Entry flow:** `main.rs` → `cli.rs` (clap) → `generate.rs` orchestrates the pipeline:
1. `gather_context()` — loads config, detects LLM provider, fetches git log + GitHub data
2. `prompt.rs` — builds system/user prompts with repo context and style examples
3. `agent.rs` — runs the agentic tool-use loop (max 25 iterations) against the LLM
4. `publish()` — optionally updates the GitHub release

**LLM abstraction:** `llm.rs` defines the `LlmClient` trait with implementations in `providers/anthropic.rs` and `providers/openai.rs`. Provider is auto-detected from model name prefix (`claude-*` → Anthropic, else OpenAI).

**Tool system:** `tools/mod.rs` dispatches tool calls from the agent loop. Tools are defined as JSON schemas sent to the LLM. The `submit_release_notes` tool is the terminal action that ends the agent loop.

**Config:** `config.rs` loads `communique.toml` (TOML) from the repo root with defaults for model, provider, emoji, link verification, etc. CLI args override config values.

## Key Patterns

- All async with tokio; `miette::Result` used throughout for rich error diagnostics
- `read_file` tool has path traversal protection (canonicalizes and checks against repo root)
- Link verification (`links.rs`) checks all URLs in output before accepting submission
- Tests use `wiremock` for HTTP mocking and `test_helpers::TempRepo` for git fixtures
- The project dogfoods itself via `release-plz.yml` to generate its own release notes
