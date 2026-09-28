---
name: communique
description: Generate, review, and publish release notes with Communique, including changelogs and package releases. Use when working with the communique CLI or communique.toml editorial configuration.
---

# Communique

Read the repository's `communique.toml`, release workflow, and existing release
style before choosing flags. Use `communique init` only when configuration is
missing. CLI flags override configuration; preserve the project's provider and
model choices unless the task calls for changing them.

## Choose the release range

Run from the target Git repository. Check the tag and previous reference before
generation; pass the baseline explicitly when automatic selection is unsuitable:

```sh
communique generate v1.2.0 v1.1.0 --draft release.json --review-report review.md
```

In a monorepo, use existing `[packages.NAME]` settings with `--package NAME`.
`--path` values replace the package's configured paths and are literal
repository-relative paths, not globs. `--tag-pattern` filters baseline tags;
`--channel stable` excludes prereleases, while the default `all` includes them.
Check the selected range printed by the command. Use `HEAD` for unreleased
changelog work, not for publishing a GitHub Release.

Generation calls an LLM and may read GitHub context, including with `--dry-run`.
Use the configured provider's credentials (`ANTHROPIC_API_KEY` or
`OPENAI_API_KEY`) through the existing secret mechanism without printing them.
`GITHUB_TOKEN` provides PR context and release access; label rules require it.
Missing history calls for fetching the needed tags or unshallowing a shallow
checkout, not substituting a guessed baseline.

## Review and edit the result

Use `--draft release.json` when the result needs review before publication.
It saves the repository, tag, target commit, title, body, changelog, and review
metadata. Edit `release_title`, `release_body`, or `changelog` in that file as
needed; preserve the target and provenance fields.

- Use `--review-report review.md` to inspect included, omitted, and uncertain
  commits. The model's coverage assessments are advisory; check unclear claims
  against their linked source changes.
- Add `--migration-guide upgrade.md` when upgrade instructions are needed.
  Verify examples and affected users before publishing.
- Put reusable audience context in `context` and writing guidance in
  `system_extra`. Label rules belong under `[rules]`; explicit inclusion wins
  over exclusion. Do not silently drop a change because its PR is inaccessible.

`generate --dry-run` skips GitHub/changelog updates and link verification; it
can still write explicitly requested drafts, reports, or output files. Review
links separately when using that mode.

## Apply the requested output

Preview an edited draft with:

```sh
communique publish release.json --dry-run
```

When publication is authorized, `communique publish release.json` updates the
existing GitHub Release without calling the model again. It checks that the
remote tag still names the saved commit. If the tag moved, regenerate and review
the notes; do not edit `target_commit` just to bypass the check. Draft generation
cannot be combined with `--github-release`.

For an authorized direct generation and publication, use `generate TAG
--github-release`. This replaces the release body. To retain hand-written text,
use `--preserve-sections` during generation (including draft generation); it
manages only the `<!-- communique:start -->` / `<!-- communique:end -->` section
and preserves the existing title. Its publish preview needs GitHub access to
merge the existing body, unlike a plain draft preview.

For local output, use `--output RELEASE_NOTES.md`, or `--changelog` to update
the version entry in `CHANGELOG.md`. `--concise` selects the short changelog
form. Publishing a saved draft does not write its changelog field to disk.
Inspect the resulting file diff or fetch the updated release to verify the
requested output.
