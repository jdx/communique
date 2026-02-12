# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible providers — auto-detects provider from model name, or configure explicitly with `--provider` / `defaults.provider`
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing to GitHub or verifying links
- Link verification — automatically checks all URLs in generated release notes and asks the AI to fix broken links
- Progress indication with spinners showing current agent iteration and tool calls
- `communique.toml` configuration file with `communique init` command to scaffold it
- Style matching — fetches recent GitHub releases and asks the AI to match their tone and formatting
- Emoji toggle — disable emoji in output via `defaults.emoji = false` in config
- Auto-generated CLI reference docs via usage-lib
- VitePress documentation site
- Support for non-tag refs (commit SHAs, branches) in tag arguments

### Fixed
- `previous_tag` now falls back to root commit when no prior tags exist, enabling first-release generation
- Ref resolution falls back to HEAD when a tag doesn't exist yet (e.g. during pre-release workflows)

### Changed
- Output is now captured via structured `submit_release_notes` tool call instead of text parsing, improving reliability

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
