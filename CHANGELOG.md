# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible providers — use any OpenAI-compatible API alongside Anthropic (`--provider`, `--base-url`)
- New agent tools: `get_issue`, `git_show`, and `get_commits` for richer repository context
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing or verifying links
- Link verification in the agent loop — broken URLs are automatically detected and sent back to the model for correction
- Retry with exponential backoff for all API calls (Anthropic, OpenAI, and GitHub)
- `--verbose` and `--quiet` flags for controlling output verbosity
- `--config` flag to specify a custom config file path
- `--output` flag to write generated notes to a file
- Text fallback parsing when the model returns prose instead of calling `submit_release_notes`
- Progress indication via `clx` spinners and status messages
- VitePress documentation site
- Auto-generated CLI reference docs via `usage-lib`

### Changed
- Tool calls from a single LLM turn are now executed concurrently for faster generation
- Existing release and recent releases are fetched in parallel
- Provider is now a validated enum (`anthropic` / `openai`) with proper config validation via `strum`/`serde`

### Fixed
- `previous_tag` now falls back to the root commit when no tags exist
- Non-tag git refs are now supported for the previous tag argument

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
