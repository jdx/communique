pub fn system_prompt(extra: Option<&str>) -> String {
    let mut prompt = r#"You are an expert technical writer generating release notes for a software project.

You have access to tools to browse the repository:
- read_file: Read file contents (path relative to repo root)
- list_files: List tracked files, optionally filtered by glob
- grep: Search file contents with ripgrep
- get_pr: Fetch GitHub PR details (title, body, labels, author)
- get_pr_diff: Fetch the diff for a GitHub PR

Use these tools to understand what changed and why. Read relevant source files, PR descriptions, and diffs to write accurate, insightful release notes.

When you are done researching, call the `submit_release_notes` tool with:
- `changelog`: A concise changelog entry using Keep a Changelog categories (### Added, ### Fixed, etc). No version header.
- `release_title`: A catchy, concise title for the GitHub release.
- `release_body`: Detailed GitHub release notes in markdown — a brief narrative summary (2-3 sentences) followed by sections covering notable changes with contributor mentions (@username) where relevant. Where it would genuinely help users understand a change, include a brief code snippet, usage example, or simple ASCII diagram — but only when it adds real clarity (e.g. a new CLI flag, a config option, or an architectural change). Don't force it.

Write clearly and concisely. Focus on what matters to users. Do NOT fabricate changes — only describe what you can verify from the git log, PRs, and source code."#.to_string();

    if let Some(extra) = extra {
        prompt.push_str("\n\n");
        prompt.push_str(extra);
    }

    prompt
}

pub fn user_prompt(
    tag: &str,
    prev_tag: &str,
    git_log: &str,
    pr_numbers: &[u64],
    changelog_entry: Option<&str>,
    existing_release: Option<&str>,
    context: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    if let Some(ctx) = context {
        parts.push(format!("## Project Context\n{ctx}"));
    }

    parts.push(format!(
        "Generate release notes for **{tag}** (previous release: {prev_tag}).\n\n\
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
