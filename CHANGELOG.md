# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible provider — use any OpenAI API-compatible model via `--provider openai` and `--base-url`, with automatic provider detection from model name
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing to GitHub or verifying links
- Automatic link verification that checks all URLs in generated notes for broken links and asks the LLM to fix them
- Progress indication with spinners and status messages during generation
- Emoji toggle (`emoji = false` in config) to suppress emoji in output
- Style matching — fetches recent GitHub releases to match your project's existing tone and formatting
- Structured output via `submit_release_notes` tool call for more reliable release note generation
- VitePress documentation site with getting started guide and configuration reference
- Auto-generated CLI reference docs via usage-lib

### Fixed
- `previous_tag` now falls back to the root commit when no prior tags exist, enabling first-release support
- Non-tag refs (branches, SHAs, unreleased versions) now resolve correctly via HEAD fallback
- TOML config parse errors now display rich diagnostics with source spans via miette

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
