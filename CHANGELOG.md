# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2](https://github.com/jdx/communique/compare/v0.1.1...v0.1.2) - 2026-02-12

### Other

- Prefix release titles with tag and use draft releases

## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12

## Added
- Multi-model LLM support with OpenAI-compatible provider — use GPT-4, Groq, Together, Ollama, or any OpenAI-compatible API via `--provider` and `--base-url` flags
- `--dry-run` (`-n`) flag to preview release notes without updating GitHub or verifying links
- `--verbose` (`-v`) and `--quiet` (`-q`) flags for controlling output verbosity
- `--config` (`-c`) flag to specify a custom config file path
- `--output` (`-o`) flag to write output to a file instead of stdout
- Link verification: generated release notes are checked for broken URLs (404s), and the LLM is asked to fix them before finalizing
- Emoji toggle via `emoji` config option (default: true) to suppress emoji in output
- Automatic retry with exponential backoff for transient API failures (429, 500, 502, 503, 529) with Retry-After header support
- New agent tools: `get_issue`, `git_show`, and `get_commits` for richer repository context
- In-memory cache for tool call results to avoid redundant API calls
- Text fallback parsing when the LLM skips the `submit_release_notes` tool call
- Style matching: fetches recent releases to guide the LLM on tone and formatting
- Parallel tool dispatch in the agent loop for faster multi-tool iterations
- Parallel fetching of existing release and recent releases during context gathering
- Progress indication showing detailed tool call info (e.g. `read_file(src/main.rs)`)

## Changed
- Provider is auto-detected from model name (`claude*` → Anthropic, everything else → OpenAI)
- Link verification now runs inside the agent loop so the LLM can fix broken URLs and resubmit
- Release body template updated with structured sections (Highlights, Breaking Changes, New Contributors, Full Changelog)
- Replaced `env_logger` with integrated `clx` ProgressLogger for cleaner log output alongside progress spinners

## Fixed
- `previous_tag` now falls back to the root commit when no tags exist
- `resolve_ref` suppresses stderr output when a ref doesn't exist

## [0.1.0](https://github.com/jdx/communique/releases/tag/v0.1.0) - 2026-02-11

### Other

- Add renovate config
- Add subcommands, communique.toml config, and release-plz
- Initial implementation of communiqué CLI
