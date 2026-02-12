# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible provider (auto-detects from model name; `claude*` → Anthropic, everything else → OpenAI)
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing or verifying links
- Link verification: automatically checks all URLs in generated notes for broken links and asks the LLM to fix them
- Emoji toggle: disable emoji in output via `communique.toml` (`emoji = false`)
- Style matching: fetches recent GitHub releases to match your project's existing tone and formatting
- Progress indication with spinners via `clx` for all long-running operations
- `communique init` subcommand to generate a starter `communique.toml` config file
- `communique.toml` configuration file with support for `system_extra`, `context`, defaults for model/provider/base_url, and more
- Structured output via `submit_release_notes` tool call for more reliable parsing
- VitePress documentation site
- Auto-generated CLI reference docs via `usage-lib`

### Fixed
- `previous_tag` now falls back to the root commit when no prior tags exist, so first releases work correctly
- Git ref resolution falls back to HEAD when a tag doesn't exist yet (e.g., during pre-release workflows)

### Changed
- LLM interaction refactored from Anthropic-only to a provider-agnostic `LlmClient` trait, enabling any OpenAI-compatible API

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
