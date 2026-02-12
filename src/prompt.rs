pub fn system_prompt(extra: Option<&str>, emoji: bool) -> String {
    let mut prompt = r#"You are an expert technical writer generating release notes for a software project.

You have access to tools to browse the repository:
- read_file: Read file contents (path relative to repo root)
- list_files: List tracked files, optionally filtered by glob
- grep: Search file contents with ripgrep
- get_pr: Fetch GitHub PR details (title, body, labels, author)
- get_pr_diff: Fetch the diff for a GitHub PR

Use these tools to understand what changed and why. Read relevant source files, PR descriptions, and diffs to write accurate, insightful release notes.

When you are done researching, call the `submit_release_notes` tool with three fields:

### `changelog`
A concise changelog entry using Keep a Changelog categories (### Added, ### Fixed, etc). No version header — just the categorized bullet points.

### `release_title`
A catchy, concise title for the GitHub release (no # prefix).

### `release_body`
Detailed GitHub release notes in markdown. Use the following template as a base, including or omitting sections as appropriate for the release:

```
<brief narrative summary — 2-3 sentences describing the release at a high level>

## Highlights
<!-- Only include for releases with multiple notable additions. Omit for small/patch releases. -->
<!-- 2-4 bullet points calling out the most important user-facing changes -->

## What's Changed
<!-- Group changes under subheadings as needed, e.g. ### Added, ### Fixed, ### Changed, ### Deprecated -->
<!-- Each item should mention the PR (@author) where relevant -->
<!-- Where it genuinely helps, include a brief code snippet, usage example, or config sample -->

## Breaking Changes
<!-- Only if applicable. List any changes that require user action to upgrade. -->

## New Contributors
<!-- List first-time contributors to the project, with a link to their first PR -->
<!-- e.g. * @username made their first contribution in #123 -->
<!-- Omit this section if there are no new contributors -->

**Full Changelog**: https://github.com/OWNER/REPO/compare/PREV_TAG...TAG
```

Adapt the template to fit the release. Small patch releases might only need a summary and a "What's Changed" section. Large releases might use all sections. Don't include empty sections.

## Guidelines

Write clearly and concisely. Focus on what matters to END USERS of the software. Do NOT fabricate changes — only describe what you can verify from the git log, PRs, and source code.

IMPORTANT: Only include changes that affect end users. Omit purely internal changes such as CI/CD pipeline updates, linter configurations, pre-commit hooks, build caching, code formatting, internal refactors, dependency updates (unless they fix a user-facing bug or add a user-facing feature), and dev tooling changes. If a release has no user-facing changes, say so briefly rather than padding the notes with internal details.

Be honest about the scope of a release. If it's a small patch with one or two fixes, say that — don't inflate it into something bigger than it is. A short, accurate release note is always better than a long, padded one."#.to_string();

    if !emoji {
        prompt.push_str("\n\nDo NOT use emoji anywhere in the output — not in headings, titles, bullet points, or prose.");
    }

    if let Some(extra) = extra {
        prompt.push_str("\n\n");
        prompt.push_str(extra);
    }

    prompt
}

pub struct UserPromptContext<'a> {
    pub tag: &'a str,
    pub prev_tag: &'a str,
    pub owner_repo: &'a str,
    pub git_log: &'a str,
    pub pr_numbers: &'a [u64],
    pub changelog_entry: Option<&'a str>,
    pub existing_release: Option<&'a str>,
    pub context: Option<&'a str>,
}

pub fn user_prompt(ctx: &UserPromptContext) -> String {
    let UserPromptContext {
        tag,
        prev_tag,
        owner_repo,
        git_log,
        pr_numbers,
        changelog_entry,
        existing_release,
        context,
    } = ctx;
    let mut parts = Vec::new();

    if let Some(ctx) = context {
        parts.push(format!("## Project Context\n{ctx}"));
    }

    parts.push(format!(
        "Generate release notes for **{tag}** (previous release: {prev_tag}).\n\
         Repository: `{owner_repo}` (https://github.com/{owner_repo})\n\n\
         ## Git Log\n```\n{git_log}\n```"
    ));

    if !pr_numbers.is_empty() {
        let prs = pr_numbers
            .iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!(
            "\n## Referenced PRs\n{prs}\n\nUse the `get_pr` and `get_pr_diff` tools to understand these changes in detail."
        ));
    }

    if let Some(entry) = changelog_entry {
        parts.push(format!(
            "\n## Existing CHANGELOG.md Entry\nHere is the current auto-generated entry — use it as a starting point and improve it:\n```\n{entry}\n```"
        ));
    }

    if let Some(body) = existing_release {
        parts.push(format!(
            "\n## Existing GitHub Release Body\nHere are the current auto-generated release notes — editorialize and improve them:\n```\n{body}\n```"
        ));
    }

    parts.push("\nBrowse the repository as needed to understand the changes, then call `submit_release_notes` with the final output.".into());

    parts.join("\n")
}
