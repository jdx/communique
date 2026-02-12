# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

### Added
- Multi-model LLM support with OpenAI-compatible provider — use OpenAI, Groq, Together, Ollama, and more via `--provider` and `--base-url` flags or config
- Dry-run mode (`--dry-run` / `-n`) to preview release notes without publishing or verifying links
- Automatic retry with exponential backoff for API calls (Anthropic, OpenAI, and GitHub), with Retry-After header support for 429s
- Link verification in the agent loop — broken URLs are sent back to the LLM for correction before finalizing
- Parallel tool dispatch in the agent loop for faster multi-tool iterations
- New agent tools: `get_issue`, `git_show`, and `get_commits` for richer repository context
- Emoji toggle (`emoji` config option) to suppress emoji in generated output
- `verify_links` config option (default: true) to control URL verification
- `match_style` config option to match tone/structure of recent releases
- Progress spinner with detailed status updates during generation
- Structured release body template with Highlights, What's Changed, Breaking Changes, and New Contributors sections
- VitePress documentation site with auto-generated CLI reference

### Fixed
- `previous_tag` now falls back to the root commit when no tags exist, enabling first-release generation
- `resolve_ref` no longer prints stderr when a ref doesn't exist

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
