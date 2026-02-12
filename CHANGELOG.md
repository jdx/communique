# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- VitePress documentation site with vaporwave theme, including getting-started guide, configuration reference, and auto-generated CLI reference
- README with installation instructions, quick start guide, and project roadmap
- Progress indication via `clx` — spinners and status messages while fetching PRs and waiting on the LLM
- `usage-lib` integration for auto-generated CLI reference docs
- `hk` pre-commit hooks with `cargo-clippy` and `cargo-fmt` linters
- Lint CI workflow (`cargo clippy` + `cargo fmt`)
- Rust build cache (`Swatinem/rust-cache`) in release-plz workflow

### Fixed
- `previous_tag` now falls back to the root commit when no tags exist, preventing failures on first release
- `resolve_ref` gracefully falls back to HEAD when a ref doesn't exist yet (e.g. unreleased version tags)
- Non-tag refs (branches, SHAs) now work correctly as version arguments

### Changed
- Agent output uses structured tool calls (`submit_release_notes`) instead of text parsing with `---SECTION_BREAK---` delimiters
- TOML config parse errors now display rich diagnostics via `miette` with source spans
- Release-plz workflow builds communique from `main` before checking out PR branch, and uses debug builds for faster CI
- Communique is now integrated into the release-plz workflow to auto-editorialize release notes and changelog entries on both releases and release PRs

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
