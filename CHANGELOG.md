# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Other

- Add generate.rs integration tests and extract shared test helpers
- Add CLAUDE.md with project guidance for Claude Code
- Add code coverage to CI ([#22](https://github.com/jdx/communique/pull/22))
- Add ripgrep to mise tools and remove has_rg guards from tests
- Add agent loop edge case and link verification fallback tests
- Update release PR title with communique output ([#23](https://github.com/jdx/communique/pull/23))
- Rename lint workflow to CI and add cargo test
- Add comprehensive test suite across all modules
- Add mock LlmClient agent loop tests
- Add multi-model LLM support with OpenAI-compatible provider
- Add release body template, dry-run mode, and link verification in agent loop
- Add link verification, emoji toggle, tool details, and prompt improvements
- Add pkl to mise tools for hk config parsing in CI
- Add hk to mise tools so it's available in CI
- Use structured tool call for release notes output
- Add hk for pre-commit hooks, lint CI, miette TOML errors, and resolve_ref fallback
- Use debug builds and add Rust cache to release-plz workflow
- Build communique from main before checking out PR branch
- Fix previous_tag to fall back to root commit when no tags exist
- Support non-tag refs and integrate communique into release-plz workflow
- Add usage-lib for auto-generated CLI reference docs
- Add progress indication via clx
- Add README with roadmap
- Add VitePress docs site with vaporwave theme
- release v0.1.0 ([#3](https://github.com/jdx/communique/pull/3))

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
