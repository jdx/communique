# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible provider — use GPT-4, Groq, Together, Ollama, or any OpenAI-compatible API alongside Anthropic Claude via `--provider` and `--base-url` flags
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing or verifying links
- Automatic link verification in generated release notes, with the LLM agent able to fix broken URLs before final output
- Emoji toggle (`emoji = true/false` in config) to suppress emoji in output
- Style matching: automatically fetches recent releases and instructs the LLM to match their tone and formatting
- New agent tools: `get_issue`, `git_show`, and `get_commits` for richer repository context
- Parallel tool dispatch — multiple tool calls in a single LLM turn now execute concurrently
- Retry with exponential backoff for transient API errors (429, 500, 502, 503, 529) with Retry-After header support
- Progress spinner with detailed status updates throughout the generation process
- Structured `submit_release_notes` tool call replaces free-form text parsing for reliable output
- VitePress documentation site

### Fixed
- Previous tag detection now falls back to root commit when no tags exist (first release)
- Non-tag refs (HEAD, branches, commit SHAs) now work as the current tag argument
- TOML parse errors now show source spans via miette for easier debugging

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
