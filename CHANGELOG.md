# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing to GitHub or verifying links
- Automatic link verification that checks all URLs in generated notes and asks the agent to fix broken links
- Emoji toggle via `defaults.emoji` config option to control emoji usage in output
- `verify_links` config option to enable/disable link verification (defaults to `true`)
- Progress indication with spinners and status messages during generation
- Structured `submit_release_notes` tool call for more reliable output from the AI agent
- Release body template with narrative summary, highlights, and changelog sections
- VitePress documentation site with getting-started guide and configuration reference
- Auto-generated CLI reference docs via usage-lib

### Fixed
- `previous_tag` now falls back to the repository root commit when no prior tags exist
- Non-tag refs (branches, commit SHAs) are now supported — `resolve_ref` falls back to HEAD if a ref doesn't exist yet
- TOML configuration errors now display precise source spans via miette for easier debugging

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
