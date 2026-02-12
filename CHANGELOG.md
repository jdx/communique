# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Progress indication with spinners and status messages via `clx` during repo discovery, git log reading, and AI generation
- VitePress documentation site with a custom vaporwave theme, guide pages, and auto-generated CLI reference
- `usage-lib` integration for auto-generated CLI reference docs (`communique usage` subcommand)
- README with installation instructions, quick start guide, and project roadmap
- `hk` pre-commit hooks with `cargo-clippy` and `cargo-fmt` linting
- Lint CI workflow using `mise run lint`
- Rust cache in release-plz CI workflow for faster builds
- `communique.toml` TOML parse errors now display rich diagnostics via `miette` with source spans

### Fixed
- `previous_tag` now falls back to the repository root commit when no tags exist, preventing errors on first release
- `resolve_ref` falls back to HEAD when a ref doesn't exist (e.g. a tag not yet created), preventing failures during release-plz workflows
- Non-tag refs (branches, commit SHAs) are now supported as version references

### Changed
- Agent output now uses a structured `submit_release_notes` tool call instead of text parsing with `---SECTION_BREAK---` delimiters, making output extraction more reliable
- Release-plz workflow builds communique from main before checking out the PR branch, ensuring a working binary is available
- Release-plz workflow now uses debug builds for speed and integrates communique to editorialize both PR bodies and GitHub releases

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
