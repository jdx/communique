# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible provider — use any OpenAI API-compatible model (GPT-4, Llama, etc.) alongside Anthropic Claude, with auto-detection from model name
- Dry-run mode (`--dry-run` / `-n`) to preview generated release notes without publishing to GitHub or verifying links
- Link verification in the agent loop — broken URLs in generated notes are automatically detected and the model is asked to fix them
- Progress indication with spinner and status messages during generation via `clx`
- Subcommands: `communique generate`, `communique init`, and `communique usage`
- `communique.toml` configuration file with support for default model, provider, base URL, emoji toggle, link verification, style matching, and custom system prompt additions
- `--provider`, `--base-url`, and `--model` CLI flags for per-invocation LLM configuration
- Structured tool call (`submit_release_notes`) for more reliable output parsing
- VitePress documentation site with CLI reference
- Auto-generated CLI reference docs via `usage-lib`

### Fixed
- `previous_tag` now falls back to the repository root commit when no prior tags exist, preventing errors on first releases
- Git ref resolution falls back to HEAD when a tag doesn't exist yet (e.g., during release-plz workflows)
- TOML config parse errors now show source spans via `miette` for better diagnostics

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
