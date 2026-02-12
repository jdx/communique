# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

Now I have a comprehensive understanding of the project and all the changes. Let me compile the release notes.

### Added
- VitePress documentation site with vaporwave theme, including getting-started guide, configuration reference, and CLI reference pages
- Progress indication via `clx` for better UX during agent execution
- Pre-commit hooks via `hk`, lint CI, miette-powered TOML error reporting, and `resolve_ref` fallback for robustness
- `usage-lib` integration for auto-generated CLI reference documentation
- README with project roadmap
- Renovate configuration for automated dependency updates
- `communique.toml` configuration file support with `init` and `generate` subcommands
- `release-plz` integration for automated releases

### Fixed
- `previous_tag` now falls back to root commit when no tags exist
- Support for non-tag refs in git range resolution
- Debug builds and Rust cache added to release-plz workflow for faster CI

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
