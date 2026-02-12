# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support: use OpenAI-compatible providers in addition to Anthropic/Claude, with auto-detection from model name
- New `--provider` and `--base-url` CLI flags for specifying LLM provider and endpoint
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing or verifying links
- Automatic link verification that detects broken URLs in generated notes and asks the model to fix them
- Progress indication with spinner and status messages during generation
- Emoji toggle via `emoji` config option in `communique.toml`
- Style matching: fetches recent GitHub releases to match tone and formatting
- Structured tool-call output (`submit_release_notes`) for more reliable results
- VitePress documentation site and auto-generated CLI reference
- New `communique.toml` config options: `provider`, `base_url`, `emoji`, `verify_links`, `match_style`

### Fixed
- `previous_tag` now falls back to the root commit when no previous tags exist
- `resolve_ref` falls back to HEAD when a ref doesn't exist (e.g., a tag not yet created)
- Support for non-tag refs (branches, commit SHAs) in version ranges

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
