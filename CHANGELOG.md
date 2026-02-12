# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support: use OpenAI, or any OpenAI-compatible provider, in addition to Anthropic Claude (`--provider`, `--base-url` flags or `communique.toml` config)
- Dry-run mode (`--dry-run` / `-n`) to preview generated notes without updating GitHub or verifying links
- Automatic link verification checks all URLs in generated release notes for broken links before publishing
- Retry with exponential backoff for API calls, including `Retry-After` header support for rate limiting
- Parallel tool dispatch — multiple tool calls from a single LLM turn now execute concurrently
- Progress indication with spinner/status while fetching PRs and waiting on the LLM
- Emoji toggle — disable emoji in output via `emoji = false` in `communique.toml`
- Style matching — fetches recent releases to match your project's existing tone and formatting
- VitePress documentation site

### Fixed
- `previous_tag` now falls back to the root commit when no prior tags exist, fixing first-release generation
- Non-tag refs (commit SHAs, branches) are now supported as the `prev_tag` argument
- Invalid `communique.toml` files now produce rich diagnostic errors via miette with source spans

### Changed
- Release notes output now uses structured tool calls instead of text parsing, improving reliability

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
