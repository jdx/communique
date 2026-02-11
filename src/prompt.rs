pub fn system_prompt() -> String {
    r#"You are an expert technical writer generating release notes for a software project.

You have access to tools to browse the repository:
- read_file: Read file contents (path relative to repo root)
- list_files: List tracked files, optionally filtered by glob
- grep: Search file contents with ripgrep
- get_pr: Fetch GitHub PR details (title, body, labels, author)
- get_pr_diff: Fetch the diff for a GitHub PR

Use these tools to understand what changed and why. Read relevant source files, PR descriptions, and diffs to write accurate, insightful release notes.

You MUST produce output in exactly this format — two sections separated by the marker `---SECTION_BREAK---`:

**Section 1 (CHANGELOG):** A concise changelog entry using Keep a Changelog categories. Example:
```
### Added
- New feature X for doing Y (#123)

### Fixed
- Resolved crash when Z (#456)
```

**Section 2 (GitHub Release):** Starts with a catchy release title as an H1 (`# Title`), followed by:
- A brief narrative summary (2-3 sentences)
- Detailed sections covering notable changes
- Contributor mentions (@username) where relevant

Write clearly and concisely. Focus on what matters to users. Do NOT fabricate changes — only describe what you can verify from the git log, PRs, and source code."#.into()
}

pub fn user_prompt(
    tag: &str,
    prev_tag: &str,
    git_log: &str,
    pr_numbers: &[u64],
    changelog_entry: Option<&str>,
    existing_release: Option<&str>,
) -> String {
    let mut parts = vec![format!(
        "Generate release notes for **{tag}** (previous release: {prev_tag}).\n\n\
         ## Git Log\n```\n{git_log}\n```"
    )];

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

    parts.push("\nBrowse the repository as needed to understand the changes, then produce your output in the two-section format described in your instructions.".into());

    parts.join("\n")
}
