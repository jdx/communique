pub fn system_prompt(extra: Option<&str>, emoji: bool) -> String {
    let mut prompt = r#"You are an expert technical writer generating release notes for a software project.

You have access to tools to browse the repository:
- read_file: Read contents of a git-tracked file (path relative to repo root)
- list_files: List tracked files, optionally filtered by glob
- grep: Search file contents with ripgrep
- get_pr: Fetch GitHub PR details (title, body, labels, author)
- get_pr_diff: Fetch the diff for a GitHub PR
- get_issue: Fetch GitHub issue details (title, body, labels, state)
- git_show: Show full details of a commit (message, author, diff)
- get_commits: List commits between refs or for a specific file path

Use these tools to understand what changed and why. Read relevant source files, PR descriptions, and diffs to write accurate, insightful release notes.

When you are done researching, call the `submit_release_notes` tool with three fields:

### `changelog`
A concise changelog entry using Keep a Changelog categories (## Added, ## Fixed, etc). No version header — just the categorized bullet points. Reference relevant PRs, issues, and commits as markdown links — e.g. `[#123](https://github.com/OWNER/REPO/pull/123)` for PRs/issues or `[abc1234](https://github.com/OWNER/REPO/commit/abc1234)` for commits.

### `release_title`
A catchy, concise title for the GitHub release (no # prefix, no version tag — the version will be prepended automatically as "vX.Y.Z: your title").

### `release_body`
Detailed GitHub release notes in markdown. Use the following template as a base, including or omitting sections as appropriate for the release:

```
<brief narrative summary — 1-2 sentences describing the release at a high level>

## Highlights
<!-- Omit this section unless the release is broad enough that readers need an executive summary. -->
<!-- Include it only for large releases with roughly 10+ distinct user-facing changes or 4+ independent headline themes. -->
<!-- If included, write 2-3 synthesis bullets that group related work; do not repeat the same bullets that appear in the categorized sections. -->

## Added / ## Fixed / ## Changed / etc.
<!-- Use top-level ## headings for each category (Added, Fixed, Changed, Deprecated, Removed). Only include categories that apply. -->
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

Adapt the template to fit the release. Small releases might only need a summary and categorized sections, regardless of whether the version is patch, minor, or major. Most releases should omit Highlights; use it only when it reduces scanning effort instead of duplicating the sections below. Don't include empty sections.

Each section needs a distinct job:
- The opening paragraph frames the release in 1-2 sentences.
- Highlights, when present, group broad themes for skimming; they should not be a second categorized changelog.
- Categorized sections carry the concrete details, PR links, authors, examples, and compatibility notes.

Avoid saying the same change three times. If a change appears in Highlights, keep the categorized bullet focused on extra detail or omit the duplicate detail entirely. Prefer fewer, denser bullets over repeated summaries.

## Guidelines

Write clearly and concisely. Focus on what matters to END USERS of the software. Do NOT fabricate changes — only describe what you can verify from the git log, PRs, and source code.

IMPORTANT: Only include changes that affect end users. Omit purely internal changes such as CI/CD pipeline updates, linter configurations, pre-commit hooks, build caching, code formatting, internal refactors, dependency updates (unless they fix a user-facing bug or add a user-facing feature), and dev tooling changes. If a release has no user-facing changes, say so briefly rather than padding the notes with internal details.

Be honest about the scope of a release. If it only has one or two user-facing changes, say that — don't inflate it into something bigger than it is. A short, accurate release note is always better than a long, padded one."#.to_string();

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
    pub is_unreleased_head: bool,
    pub changelog_entry: Option<&'a str>,
    pub existing_release: Option<&'a str>,
    pub context: Option<&'a str>,
    pub recent_releases: &'a [(String, String)],
}

pub fn user_prompt(ctx: &UserPromptContext) -> String {
    let UserPromptContext {
        tag,
        prev_tag,
        owner_repo,
        git_log,
        pr_numbers,
        is_unreleased_head,
        changelog_entry,
        existing_release,
        context,
        recent_releases,
    } = ctx;
    let mut parts = Vec::new();

    if let Some(ctx) = context {
        parts.push(format!("## Project Context\n{ctx}"));
    }

    let release_request = if *is_unreleased_head {
        format!("Generate release notes for unreleased changes since {prev_tag}.")
    } else {
        format!("Generate release notes for **{tag}** (previous release: {prev_tag}).")
    };

    parts.push(format!(
        "{release_request}\n\
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
        if *is_unreleased_head {
            parts.push(format!(
                "\n## Existing Unreleased CHANGELOG.md Draft\nHere is the current draft unreleased material — reconcile it with the generated notes and improve it. Produce only the changelog section body, without a nested `## [Unreleased]` or `## Unreleased` heading:\n```\n{entry}\n```"
            ));
        } else {
            parts.push(format!(
                "\n## Existing CHANGELOG.md Entry\nHere is the current auto-generated entry — use it as a starting point and improve it:\n```\n{entry}\n```"
            ));
        }
    }

    if let Some(body) = existing_release {
        parts.push(format!(
            "\n## Existing GitHub Release Body\nHere are the current auto-generated release notes — editorialize and improve them:\n```\n{body}\n```"
        ));
    }

    if !recent_releases.is_empty() {
        let mut section = String::from(
            "\n## Style Reference (Recent Releases)\nMatch the tone, structure, and formatting of these recent release notes:\n",
        );
        for (tag_name, body) in *recent_releases {
            let truncated = if body.len() > 3072 {
                let mut end = 3072;
                while !body.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}...\n[truncated]", &body[..end])
            } else {
                body.clone()
            };
            section.push_str(&format!("\n### {tag_name}\n```\n{truncated}\n```\n"));
        }
        parts.push(section);
    }

    parts.push("\nBrowse the repository as needed to understand the changes, then call `submit_release_notes` with the final output.".into());

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_default() {
        let prompt = system_prompt(None, true);
        assert!(prompt.contains("submit_release_notes"));
        assert!(prompt.contains("Keep a Changelog"));
        assert!(prompt.contains("10+ distinct user-facing changes"));
        assert!(prompt.contains("do not repeat the same bullets"));
        assert!(prompt.contains("Each section needs a distinct job"));
        assert!(!prompt.contains("Do NOT use emoji"));
    }

    #[test]
    fn test_system_prompt_no_emoji() {
        let prompt = system_prompt(None, false);
        assert!(prompt.contains("Do NOT use emoji"));
    }

    #[test]
    fn test_system_prompt_with_extra() {
        let prompt = system_prompt(Some("Always mention cats"), true);
        assert!(prompt.contains("Always mention cats"));
    }

    #[test]
    fn test_user_prompt_minimal() {
        let prompt = user_prompt(&UserPromptContext {
            tag: "v1.0.0",
            prev_tag: "v0.9.0",
            owner_repo: "jdx/communique",
            git_log: "abc1234 feat: add feature",
            pr_numbers: &[],
            is_unreleased_head: false,
            changelog_entry: None,
            existing_release: None,
            context: None,
            recent_releases: &[],
        });
        assert!(prompt.contains("v1.0.0"));
        assert!(prompt.contains("v0.9.0"));
        assert!(prompt.contains("jdx/communique"));
        assert!(prompt.contains("abc1234 feat: add feature"));
        assert!(!prompt.contains("Referenced PRs"));
        assert!(!prompt.contains("CHANGELOG.md"));
        assert!(!prompt.contains("Style Reference"));
    }

    #[test]
    fn test_user_prompt_with_prs() {
        let prompt = user_prompt(&UserPromptContext {
            tag: "v1.0.0",
            prev_tag: "v0.9.0",
            owner_repo: "jdx/communique",
            git_log: "abc1234 feat (#42)",
            pr_numbers: &[42, 99],
            is_unreleased_head: false,
            changelog_entry: None,
            existing_release: None,
            context: None,
            recent_releases: &[],
        });
        assert!(prompt.contains("Referenced PRs"));
        assert!(prompt.contains("#42"));
        assert!(prompt.contains("#99"));
        assert!(prompt.contains("get_pr"));
    }

    #[test]
    fn test_user_prompt_full() {
        let prompt = user_prompt(&UserPromptContext {
            tag: "v2.0.0",
            prev_tag: "v1.0.0",
            owner_repo: "jdx/communique",
            git_log: "def5678 fix: bug",
            pr_numbers: &[10],
            is_unreleased_head: false,
            changelog_entry: Some("### Fixed\n- Bug fix"),
            existing_release: Some("Previous release body"),
            context: Some("This is a CLI tool for release notes."),
            recent_releases: &[("v1.0.0".into(), "Release 1.0 notes".into())],
        });
        assert!(prompt.contains("Project Context"));
        assert!(prompt.contains("CLI tool for release notes"));
        assert!(prompt.contains("CHANGELOG.md Entry"));
        assert!(prompt.contains("Bug fix"));
        assert!(prompt.contains("Existing GitHub Release Body"));
        assert!(prompt.contains("Previous release body"));
        assert!(prompt.contains("Style Reference"));
        assert!(prompt.contains("Release 1.0 notes"));
    }

    #[test]
    fn test_user_prompt_recent_releases_truncation() {
        let long_body = "x".repeat(5000);
        let prompt = user_prompt(&UserPromptContext {
            tag: "v2.0.0",
            prev_tag: "v1.0.0",
            owner_repo: "test/repo",
            git_log: "abc init",
            pr_numbers: &[],
            is_unreleased_head: false,
            changelog_entry: None,
            existing_release: None,
            context: None,
            recent_releases: &[("v1.0.0".into(), long_body)],
        });
        assert!(prompt.contains("[truncated]"));
    }

    #[test]
    fn test_user_prompt_unreleased_head_labels_changes_without_head() {
        let prompt = user_prompt(&UserPromptContext {
            tag: "HEAD",
            prev_tag: "v1.0.0",
            owner_repo: "test/repo",
            git_log: "abc1234 feat: draft feature",
            pr_numbers: &[],
            is_unreleased_head: true,
            changelog_entry: None,
            existing_release: None,
            context: None,
            recent_releases: &[],
        });

        assert!(prompt.contains("Generate release notes for unreleased changes since v1.0.0."));
        assert!(!prompt.contains("**HEAD**"));
    }

    #[test]
    fn test_user_prompt_tagged_mode_preserves_tagged_phrase() {
        let prompt = user_prompt(&UserPromptContext {
            tag: "v2.0.0",
            prev_tag: "v1.0.0",
            owner_repo: "test/repo",
            git_log: "abc1234 feat: tagged feature",
            pr_numbers: &[],
            is_unreleased_head: false,
            changelog_entry: None,
            existing_release: None,
            context: None,
            recent_releases: &[],
        });

        assert!(
            prompt.contains("Generate release notes for **v2.0.0** (previous release: v1.0.0).")
        );
    }

    #[test]
    fn test_user_prompt_unreleased_changelog_entry_is_draft_material() {
        let prompt = user_prompt(&UserPromptContext {
            tag: "HEAD",
            prev_tag: "v1.0.0",
            owner_repo: "test/repo",
            git_log: "abc1234 feat: draft feature",
            pr_numbers: &[],
            is_unreleased_head: true,
            changelog_entry: Some("### Changed\n- Old draft"),
            existing_release: None,
            context: None,
            recent_releases: &[],
        });

        assert!(prompt.contains("Existing Unreleased CHANGELOG.md Draft"));
        assert!(prompt.contains("draft unreleased material"));
        assert!(prompt.contains("without a nested `## [Unreleased]` or `## Unreleased` heading"));
        assert!(!prompt.contains("Existing CHANGELOG.md Entry"));
        assert!(!prompt.contains("**HEAD**"));
    }

    #[test]
    fn test_user_prompt_recent_releases_truncation_on_char_boundary() {
        // Ensure truncation does not panic when the 3072-byte cutoff lands
        // inside a multi-byte UTF-8 character (e.g. the em-dash '—', 3 bytes).
        let mut body = "a".repeat(3070);
        body.push('—');
        body.push_str(&"b".repeat(2000));
        let prompt = user_prompt(&UserPromptContext {
            tag: "v2.0.0",
            prev_tag: "v1.0.0",
            owner_repo: "test/repo",
            git_log: "abc init",
            pr_numbers: &[],
            is_unreleased_head: false,
            changelog_entry: None,
            existing_release: None,
            context: None,
            recent_releases: &[("v1.0.0".into(), body)],
        });
        assert!(prompt.contains("[truncated]"));
    }
}
