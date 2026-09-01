# Communiqué Development Guide

## mbx build cache

`mise install` installs mbx 1.3. `mise run` activates the project's transparent
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
