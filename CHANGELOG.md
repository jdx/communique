# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible provider — use any OpenAI API-compatible model by setting `--provider openai` or auto-detect from model name
- Dry-run mode (`--dry-run` / `-n`) to preview generated release notes without publishing to GitHub or verifying links
- Automatic link verification — all URLs in generated release notes are checked for broken links before submission
- Emoji toggle — disable emoji in output via `communique.toml` (`emoji = false`)
- Style matching — automatically fetches recent releases to match existing tone and formatting
- Progress indication via spinners showing current agent status
- `communique init` subcommand to scaffold a `communique.toml` config file
- Structured tool call (`submit_release_notes`) for reliable output parsing
- Release body template included in the system prompt for consistent formatting
- Configurable `base_url` for self-hosted or proxy LLM endpoints

### Fixed
- `previous_tag` now falls back to the root commit when no prior tags exist
- Git ref resolution falls back to HEAD when a tag hasn't been created yet

### Changed
- Provider is auto-detected from model name (`claude*` → Anthropic, everything else → OpenAI)
- API key resolution is provider-aware (`ANTHROPIC_API_KEY` for Anthropic, `OPENAI_API_KEY` or `LLM_API_KEY` for OpenAI)

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
