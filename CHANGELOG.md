# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.9](https://github.com/jdx/communique/compare/v0.1.8...v0.1.9) - 2026-02-20

### Fixed

- *(deps)* update rust crate reqwest to 0.13 ([#57](https://github.com/jdx/communique/pull/57))
- normalize release title to "vX.Y.Z: description" format ([#55](https://github.com/jdx/communique/pull/55))

### Other

- *(deps)* update rust crate clap to v4.5.60 ([#59](https://github.com/jdx/communique/pull/59))
- *(deps)* pin dependencies ([#58](https://github.com/jdx/communique/pull/58))
- *(deps)* update rust crate futures-util to v0.3.32 ([#56](https://github.com/jdx/communique/pull/56))
- run render and lint-fix when creating release PR ([#53](https://github.com/jdx/communique/pull/53))

## [0.1.8](https://github.com/jdx/communique/releases/tag/v0.1.8) - 2026-02-18

### Added

- Extracted communique into a separate `enhance-release` CI job that runs after publish, making it independently re-runnable and removing `continue-on-error` so failures are visible ([#51](https://github.com/jdx/communique/pull/51))

### Fixed

- Propagated errors from the list releases API call in the `get_release_by_tag` fallback instead of silently swallowing them, fixing misleading "No GitHub release found" messages ([#49](https://github.com/jdx/communique/pull/49))

## [0.1.7](https://github.com/jdx/communique/compare/v0.1.6...v0.1.7) - 2026-02-17

### Other

- Fix draft release tag fix failing due to API eventual consistency ([#47](https://github.com/jdx/communique/pull/47))

## [0.1.6](https://github.com/jdx/communique/releases/tag/v0.1.6) - 2026-02-12

### Fixed
- Fixed draft release tags getting an `untagged-*` placeholder, which prevented pre-built binaries from being uploaded to GitHub releases ([#45](https://github.com/jdx/communique/pull/45))## [0.1.5](https://github.com/jdx/communique/releases/tag/v0.1.5) - 2026-02-12

### Changed
- Regenerated CLI documentation to reflect flags and options added in prior releases ([#43](https://github.com/jdx/communique/pull/43))

## [0.1.4](https://github.com/jdx/communique/releases/tag/v0.1.4) - 2026-02-12

### Changed
- Increased retry resilience for transient API failures — retries now use 10 attempts with a 1s initial delay and 60s max backoff (up from 5 attempts / 500ms / 30s), improving reliability during API outages ([#41](https://github.com/jdx/communique/pull/41))

### Fixed
- Fixed double tag prefix in release PR titles where both the workflow and generate logic prepended the tag ([#41](https://github.com/jdx/communique/pull/41))## [0.1.3](https://github.com/jdx/communique/releases/tag/v0.1.3) - 2026-02-12

### Fixed
- Fix draft release lookup — `get_release_by_tag` now falls back to listing releases when the `/releases/tags` endpoint returns 404, since that endpoint doesn't return draft releases ([f201231](https://github.com/jdx/communique/commit/f2012318af310105f0a2517b2c6ad03ce684f176))
- Fix double tag prefix in PR titles where the workflow and `generate.rs` both prepended the tag, resulting in `v0.1.3: v0.1.3: ...` ([#39](https://github.com/jdx/communique/pull/39))

### Changed
- Updated GitHub Actions documentation to show `--changelog` usage, the required `git fetch --tags` step, and simplified workflow examples ([f201231](https://github.com/jdx/communique/commit/f2012318af310105f0a2517b2c6ad03ce684f176))

### Removed
- macOS x86_64 pre-built binaries are no longer published; only aarch64 (Apple Silicon) binaries are provided for macOS ([2861482](https://github.com/jdx/communique/commit/28614828b1561ae198943e9f290b4c27be4c8a38))## [0.1.2](https://github.com/jdx/communique/compare/v0.1.1...v0.1.2) - 2026-02-12

### Other

- Prefix release titles with tag and use draft releases## [0.1.1](https://github.com/jdx/communique/compare/v0.1.0...v0.1.1) - 2026-02-12## Added
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
