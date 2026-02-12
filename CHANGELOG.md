# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support: use OpenAI, or any OpenAI-compatible API, in addition to Anthropic Claude — auto-detected from model name or set via `--provider` / config
- Dry-run mode (`--dry-run` / `-n`): preview generated release notes without publishing to GitHub or verifying links
- Link verification: automatically checks all URLs in generated notes and asks the model to fix broken links
- Progress indication with spinner and status messages while fetching PRs and waiting on the LLM
- VitePress documentation site with getting started guide and configuration reference
- Auto-generated CLI reference docs via usage-lib
- Emoji toggle (`emoji` config option) to control emoji usage in output
- Style matching (`match_style` config option) fetches recent releases to match your project's tone
- New config options: `provider`, `base_url`, `emoji`, `verify_links`, `match_style`
- Support for non-tag refs (branches, commit SHAs) as version arguments

### Fixed
- `previous_tag` now falls back to the repository root commit when no prior tags exist, fixing first-release generation
- Git ref resolution falls back to HEAD when a tag doesn't exist yet (e.g. during pre-release workflows)

### Changed
- Release notes output now uses structured tool calls instead of text parsing, improving reliability

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
