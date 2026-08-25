# Communiqué Development Guide

## mbx build cache

Compilation-heavy mise tasks and hk checks use `mbx`. If an mbx command fails
or creates a development papercut, rerun the exact equivalent `cargo` command
from `CONTRIBUTING.md`; this unblocks work without weakening the check. If Cargo
succeeds, surface the mismatch and recommend a
[mr-boxington Discussion](https://github.com/jdx/mr-boxington/discussions) with
the repository and commit, OS, `mbx --version`, both commands and outputs, the
cache summary, and `MBX_BYPASS_LOG` details when relevant. Do not silently make
Cargo the permanent path, and do not post externally without user authorization.

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
