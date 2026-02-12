mod agent;
mod anthropic;
mod cli;
mod config;
mod error;
mod git;
mod github;
mod output;
mod prompt;
mod tools;
mod usage;

use std::time::Duration;

use clap::Parser;
use clx::progress::{ProgressJobBuilder, ProgressStatus};
use log::info;
use miette::IntoDiagnostic;

use cli::{Cli, Command};
use config::Config;

#[tokio::main]
async fn main() -> miette::Result<()> {
    env_logger::init();
    clx::progress::set_interval(Duration::from_millis(100));
    if !console::user_attended_stderr() {
        clx::progress::set_output(clx::progress::ProgressOutput::Text);
    }

    let cli = Cli::parse();

    let result = match cli.command {
        Command::Usage(usage) => usage.run(),
        Command::Init { force } => cmd_init(force),
        Command::Generate {
            tag,
            prev_tag,
            github_release,
            concise,
            repo,
            model,
            max_tokens,
        } => {
            cmd_generate(
                tag,
                prev_tag,
                github_release,
                concise,
                repo,
                model,
                max_tokens,
            )
            .await
        }
    };

    clx::progress::flush();
    result
}

fn cmd_init(force: bool) -> miette::Result<()> {
    let repo_root = git::repo_root()?;
    let path = repo_root.join("communique.toml");

    if path.exists() && !force {
        return Err(error::Error::Config(format!(
            "{} already exists (use --force to overwrite)",
            path.display()
        )))
        .into_diagnostic();
    }

    xx::file::write(&path, Config::template())?;
    eprintln!("Wrote {}", path.display());
    Ok(())
}

async fn cmd_generate(
    tag: String,
    prev_tag: Option<String>,
    github_release: bool,
    concise: bool,
    repo: Option<String>,
    model: Option<String>,
    max_tokens: Option<u32>,
) -> miette::Result<()> {
    // Validate API key
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| error::Error::Anthropic("ANTHROPIC_API_KEY not set".into()))?;

    let github_token = std::env::var("GITHUB_TOKEN").ok();

    if github_release && github_token.is_none() {
        return Err(error::Error::GitHub(
            "GITHUB_TOKEN is required for --github-release".into(),
        ))
        .into_diagnostic();
    }

    let job = ProgressJobBuilder::new()
        .body("{{spinner()}} {{message | flex}}")
        .prop("message", "Discovering repository...")
        .start();

    // Discover repo
    let repo_root = git::repo_root()?;
    info!("repo root: {}", repo_root.display());

    // Load config
    let config = Config::load(&repo_root)?.unwrap_or_default();
    let defaults = config.defaults.unwrap_or_default();

    // Merge: CLI flags > config defaults > hardcoded defaults
    let model = model
        .or(defaults.model)
        .unwrap_or_else(|| "claude-opus-4-6".into());
    let max_tokens = max_tokens.or(defaults.max_tokens).unwrap_or(4096);
    let owner_repo = match repo.or(defaults.repo) {
        Some(r) => r,
        None => git::detect_remote(&repo_root)?,
    };
    info!("repo: {owner_repo}");

    // Resolve previous tag
    let prev_tag = match prev_tag {
        Some(t) => t,
        None => git::previous_tag(&repo_root, &tag)?,
    };
    info!("range: {prev_tag}..{tag}");

    // Get git log and extract PR numbers
    job.prop("message", &format!("Reading git log {prev_tag}..{tag}..."));
    let git_log = git::log_between(&repo_root, &prev_tag, &tag)?;
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
    job.prop("message", "Fetching existing release context...");
    let changelog_entry = read_changelog_entry(&repo_root, &tag);
    let existing_release = if let Some(gh) = &github_client {
        match gh.get_release_by_tag(&tag).await? {
            Some(r) => r.body,
            None => None,
        }
    } else {
        None
    };

    // Build prompts
    let system = prompt::system_prompt(config.system_extra.as_deref());
    let user_msg = prompt::user_prompt(
        &tag,
        &prev_tag,
        &git_log,
        &pr_numbers,
        changelog_entry.as_deref(),
        existing_release.as_deref(),
        config.context.as_deref(),
    );

    // Run agent
    job.prop("message", "Generating release notes...");
    let anthropic = anthropic::AnthropicClient::new(api_key, model, max_tokens);
    let tool_defs = tools::all_definitions(github_client.is_some());

    let parsed = agent::run(
        &anthropic,
        &system,
        &user_msg,
        tool_defs,
        &repo_root,
        github_client.as_ref(),
        &job,
    )
    .await?;

    // Update GitHub release if requested
    if github_release {
        job.prop("message", &format!("Updating GitHub release for {tag}..."));
        let gh = github_client.as_ref().unwrap();
        match gh.get_release_by_tag(&tag).await? {
            Some(release) => {
                gh.update_release(
                    release.id,
                    Some(&parsed.release_title),
                    Some(&parsed.release_body),
                )
                .await?;
            }
            None => {
                job.set_status(ProgressStatus::Warn);
                job.prop(
                    "message",
                    &format!("No GitHub release found for {tag} — skipping update"),
                );
                clx::progress::flush();
                return Ok(());
            }
        }
    }

    job.set_status(ProgressStatus::Done);
    job.prop("message", "Done");
    clx::progress::flush();

    // Output
    if concise {
        println!("{}", parsed.changelog);
    } else {
        println!("# {}\n\n{}", parsed.release_title, parsed.release_body);
    }

    Ok(())
}

/// Try to extract the entry for `tag` from CHANGELOG.md.
fn read_changelog_entry(repo_root: &std::path::Path, tag: &str) -> Option<String> {
    let path = repo_root.join("CHANGELOG.md");
    let contents = xx::file::read_to_string(&path).ok()?;

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
