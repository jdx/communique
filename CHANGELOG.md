# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible provider — use any OpenAI API-compatible model alongside Anthropic Claude via `--provider` or config
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing to GitHub or verifying links
- Automatic link verification — broken URLs in generated notes are detected and sent back to the model for correction
- Progress indication with spinners showing current operation (fetching PRs, generating notes, verifying links)
- Style matching — fetches recent GitHub releases to match tone and formatting conventions
- Emoji toggle via `communique.toml` (`emoji = false` to suppress emoji in output)
- VitePress documentation site with vaporwave theme
- Auto-generated CLI reference docs via usage-lib
- README with project roadmap
- Structured `submit_release_notes` tool call for cleaner output parsing

### Fixed
- `previous_tag` now falls back to the root commit when no prior tags exist, enabling first-release generation
- Non-tag refs (branches, commit SHAs) are now supported as version arguments
- TOML config parse errors now display rich diagnostics with source spans via miette

### Changed
- Release notes output now uses a structured tool call (`submit_release_notes`) instead of text parsing with section break markers

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
