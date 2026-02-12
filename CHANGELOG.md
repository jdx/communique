# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible provider — use any OpenAI API-compatible model via `--provider openai` or auto-detect from model name
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing or verifying links
- Link verification in the agent loop — broken URLs are automatically detected and the model is asked to fix them
- Configurable emoji toggle (`emoji` in `communique.toml` defaults) to suppress emoji in output
- Progress indication via spinner/status while fetching PRs and waiting on the LLM
- VitePress documentation site
- `communique init` and `communique generate` subcommands with `communique.toml` configuration file
- Style matching from recent releases — the agent reads your last 2 releases for tone/format consistency
- Structured tool call (`submit_release_notes`) for cleaner, more reliable output parsing

### Changed
- Tool calls from a single LLM turn are now executed concurrently, speeding up iterations with multiple GitHub API calls

### Fixed
- `previous_tag` now falls back to the root commit when no prior tags exist, instead of erroring
- Git ref resolution falls back to HEAD when a tag doesn't exist yet (e.g. during release-plz workflows)

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
