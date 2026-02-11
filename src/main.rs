mod agent;
mod anthropic;
mod cli;
mod error;
mod git;
mod github;
mod output;
mod prompt;
mod tools;

use std::fs;

use clap::Parser;
use log::info;
use miette::IntoDiagnostic;

use cli::Cli;

#[tokio::main]
async fn main() -> miette::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    // Validate API key
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| error::Error::Anthropic("ANTHROPIC_API_KEY not set".into()))?;

    let github_token = std::env::var("GITHUB_TOKEN").ok();

    if cli.github_release && github_token.is_none() {
        return Err(error::Error::GitHub(
            "GITHUB_TOKEN is required for --github-release".into(),
        ))
        .into_diagnostic();
    }

    // Discover repo
    let repo_root = git::repo_root()?;
    info!("repo root: {}", repo_root.display());

    let owner_repo = match &cli.repo {
        Some(r) => r.clone(),
        None => git::detect_remote(&repo_root)?,
    };
    info!("repo: {owner_repo}");

    // Resolve previous tag
    let prev_tag = match &cli.prev_tag {
        Some(t) => t.clone(),
        None => git::previous_tag(&repo_root, &cli.tag)?,
    };
    info!("range: {prev_tag}..{}", cli.tag);

    // Get git log and extract PR numbers
    let git_log = git::log_between(&repo_root, &prev_tag, &cli.tag)?;
    let pr_numbers = git::extract_pr_numbers(&git_log);
    info!(
        "found {} commits, {} PRs",
        git_log.lines().count(),
        pr_numbers.len()
    );

    // Build GitHub client if token available
    let github_client = github_token
        .as_ref()
        .map(|token| github::GitHubClient::new(token.clone(), &owner_repo))
        .transpose()?;

    // Fetch existing context
    let changelog_entry = read_changelog_entry(&repo_root, &cli.tag);
    let existing_release = if let Some(gh) = &github_client {
        match gh.get_release_by_tag(&cli.tag).await? {
            Some(r) => r.body,
            None => None,
        }
    } else {
        None
    };

    // Build prompts
    let system = prompt::system_prompt();
    let user_msg = prompt::user_prompt(
        &cli.tag,
        &prev_tag,
        &git_log,
        &pr_numbers,
        changelog_entry.as_deref(),
        existing_release.as_deref(),
    );

    // Run agent
    let anthropic = anthropic::AnthropicClient::new(api_key, cli.model.clone(), cli.max_tokens);
    let tool_defs = tools::all_definitions(github_client.is_some());

    let raw = agent::run(
        &anthropic,
        &system,
        &user_msg,
        tool_defs,
        &repo_root,
        github_client.as_ref(),
    )
    .await?;

    // Parse output
    let parsed = output::parse(&raw)?;

    // Output
    if cli.concise {
        println!("{}", parsed.changelog);
    } else {
        println!("# {}\n\n{}", parsed.release_title, parsed.release_body);
    }

    // Update GitHub release if requested
    if cli.github_release {
        let gh = github_client.as_ref().unwrap();
        match gh.get_release_by_tag(&cli.tag).await? {
            Some(release) => {
                gh.update_release(
                    release.id,
                    Some(&parsed.release_title),
                    Some(&parsed.release_body),
                )
                .await?;
                eprintln!("Updated GitHub release for {}", cli.tag);
            }
            None => {
                eprintln!(
                    "Warning: no GitHub release found for {} — skipping update",
                    cli.tag
                );
            }
        }
    }

    Ok(())
}

/// Try to extract the entry for `tag` from CHANGELOG.md.
fn read_changelog_entry(repo_root: &std::path::Path, tag: &str) -> Option<String> {
    let path = repo_root.join("CHANGELOG.md");
    let contents = fs::read_to_string(&path).ok()?;

    // Look for a section headed by this version (with or without the `v` prefix)
    let version = tag.strip_prefix('v').unwrap_or(tag);
    let header_pattern = format!("## [{version}]");
    let alt_pattern = format!("## {version}");

    let start = contents
        .find(&header_pattern)
        .or_else(|| contents.find(&alt_pattern))?;

    // Find the next `## ` header after the start
    let rest = &contents[start..];
    let end = rest[3..]
        .find("\n## ")
        .map(|i| start + 3 + i)
        .unwrap_or(contents.len());

    Some(contents[start..end].trim().to_string())
}
