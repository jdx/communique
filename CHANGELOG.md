# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Progress indication with spinner and status messages during generation
- Link verification for generated release notes (checks for broken URLs)
- Emoji toggle via `emoji` config option in `communique.toml`
- `verify_links` config option to control automatic link checking
- Support for non-tag refs (commit SHAs, branches) as the previous release reference
- Fallback to root commit when no previous tags exist, capturing full history
- Structured `submit_release_notes` tool call for more reliable AI output
- Improved TOML config error reporting with highlighted source spans via miette
- `usage` subcommand for auto-generated CLI reference
- Documentation website (VitePress)

### Fixed
- Previous tag detection now gracefully handles repos with no tags
- Ref resolution falls back to HEAD when a tag hasn't been created yet

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
