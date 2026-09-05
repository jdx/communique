# Communiqué Development Guide

## mbx build cache

`mise install` installs mbx 1.4. `mise run` activates the project's transparent
Cargo wrapper, so compilation-heavy mise tasks and hk checks use ordinary
`cargo` commands. Standalone Cargo commands require an activated mise shell. If
the wrapper fails or creates a development papercut, rerun the exact equivalent
command from `CONTRIBUTING.md` with `MBX_DISABLE=1`; this unblocks work without
weakening the check. If bypassed Cargo succeeds, surface the mismatch and recommend a
[mr-boxington Discussion](https://github.com/jdx/mr-boxington/discussions) with
the repository and commit, OS, `mbx --version`, `mbx doctor`, and both commands
and outputs. Redact secrets, absolute cache paths, remote URLs, namespaces, and
other sensitive or identifying details. Do not permanently disable the wrapper,
and do not post externally without user authorization.

## Conventional Commits

Pull request titles must use
`<type>[optional scope][optional !]: <description>`; intermediate commit
subjects should use the same format. Start descriptions with a lowercase
character and keep them concise and imperative. Use `!` for a breaking change and explain it with a
`BREAKING CHANGE:` footer.

Allowed types are `chore`, `ci`, `docs`, `feat`, `fix`, `perf`, `refactor`,
`revert`, `security`, `style`, and `test`.

CI validates the pull request title and re-runs when it is edited. Intermediate
commit subjects are not checked because pull requests are squash-merged. CI
mechanically checks the allowed type, syntax, and lowercase-leading description;
imperative mood and breaking-change details remain review rules.

## Project

Communiqué is a Rust CLI that generates AI-powered editorialized release notes.
It uses an agentic loop in which an LLM reads repository context through tools
and produces structured notes through the `submit_release_notes` tool.

## Commands

- `cargo build` — build the project
- `cargo test` — run all tests
- `cargo test <test_name>` — run one test
- `cargo clippy` — lint
- `cargo fmt` — format
- `mise run lint` — run every linter through hk

## Architecture

The entry flow is `main.rs` → `cli.rs` → `generate.rs`. Generation gathers
configuration and repository context, builds prompts, runs the agent loop, and
optionally publishes the result.

- `llm.rs` defines `LlmClient`; providers live under `providers/`.
- `tools/mod.rs` dispatches the tools exposed to the agent.
- `config.rs` loads `communique.toml`; CLI arguments override its defaults.

Use `miette::Result` for diagnostics and tokio for asynchronous work. Tests use
`wiremock` and `test_helpers::TempRepo`. Preserve the path-traversal checks in
the `read_file` tool and URL validation in `links.rs`.

## Dependency Updates

- Use the lowest compatibility-significant specificity in `Cargo.toml` (for example, `"1"` for stable 1.x dependencies).
- When the existing manifest requirement accepts a routine dependency update, change only `Cargo.lock`.
- Keep lockfile updates focused and avoid unrelated transitive dependency churn.

## GitHub Interactions

When AI contributes GitHub content—including a pull request description, review, pull request
comment, or discussion post—append this disclosure:

`*AI-assisted — Tool: <tool>; model: <provider>/<model>; version: <version-or-unavailable>.*`

Use the exact model and version identifiers exposed by the runtime. Never infer or guess them; use
`unavailable` when either value is not exposed.
