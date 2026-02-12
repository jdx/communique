use std::path::{Path, PathBuf};
use std::sync::Arc;

use clx::progress::{ProgressJob, ProgressJobBuilder, ProgressStatus};
use log::info;

use crate::config::Defaults;
use crate::output::ParsedOutput;
use crate::{agent, anthropic, config, git, github, prompt, tools};

pub struct GenerateOptions {
    pub tag: String,
    pub prev_tag: Option<String>,
    pub github_release: bool,
    pub concise: bool,
    pub dry_run: bool,
    pub repo: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
}

struct Context {
    repo_root: PathBuf,
    #[allow(dead_code)]
    owner_repo: String,
    tag: String,
    prev_tag: String,
    model: String,
    max_tokens: u32,
    api_key: String,
    defaults: Defaults,
    system_extra: Option<String>,
    context: Option<String>,
    github_client: Option<github::GitHubClient>,
}

pub async fn run(opts: GenerateOptions) -> miette::Result<()> {
    let job = ProgressJobBuilder::new()
        .body("{{spinner()}} {{message | flex}}")
        .prop("message", "Discovering repository...")
        .start();

    let ctx = gather_context(&opts, &job).await?;
    let parsed = generate_notes(&ctx, opts.dry_run, &job).await?;
    publish(&opts, &ctx, &parsed, &job).await?;

    job.set_status(ProgressStatus::Done);
    job.prop("message", "Done");
    clx::progress::flush();

    if opts.concise {
        println!("{}", parsed.changelog);
    } else {
        println!("# {}\n\n{}", parsed.release_title, parsed.release_body);
    }

    Ok(())
}

async fn gather_context(opts: &GenerateOptions, job: &Arc<ProgressJob>) -> miette::Result<Context> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| crate::error::Error::Anthropic("ANTHROPIC_API_KEY not set".into()))?;

    let github_token = std::env::var("GITHUB_TOKEN").ok();

    if opts.github_release && github_token.is_none() {
        return Err(crate::error::Error::GitHub(
            "GITHUB_TOKEN is required for --github-release".into(),
        ))?;
    }

    let repo_root = git::repo_root()?;
    info!("repo root: {}", repo_root.display());

    let config = config::Config::load(&repo_root)?.unwrap_or_default();
    let defaults = config.defaults.unwrap_or_default();

    let model = opts
        .model
        .clone()
        .or(defaults.model.clone())
        .unwrap_or_else(|| "claude-opus-4-6".into());
    let max_tokens = opts.max_tokens.or(defaults.max_tokens).unwrap_or(4096);
    let owner_repo = match opts.repo.clone().or(defaults.repo.clone()) {
        Some(r) => r,
        None => git::detect_remote(&repo_root)?,
    };
    info!("repo: {owner_repo}");

    let prev_tag = match &opts.prev_tag {
        Some(t) => t.clone(),
        None => git::previous_tag(&repo_root, &opts.tag)?,
    };
    info!("range: {prev_tag}..{}", opts.tag);

    job.prop(
        "message",
        &format!("Reading git log {prev_tag}..{}...", opts.tag),
    );

    let github_client = github_token
        .as_ref()
        .map(|token| github::GitHubClient::new(token.clone(), &owner_repo))
        .transpose()?;

    Ok(Context {
        repo_root,
        owner_repo,
        tag: opts.tag.clone(),
        prev_tag,
        model,
        max_tokens,
        api_key,
        defaults,
        system_extra: config.system_extra,
        context: config.context,
        github_client,
    })
}

async fn generate_notes(
    ctx: &Context,
    dry_run: bool,
    job: &Arc<ProgressJob>,
) -> miette::Result<ParsedOutput> {
    let git_log = git::log_between(&ctx.repo_root, &ctx.prev_tag, &ctx.tag)?;
    let pr_numbers = git::extract_pr_numbers(&git_log);
    info!(
        "found {} commits, {} PRs",
        git_log.lines().count(),
        pr_numbers.len()
    );

    job.prop("message", "Fetching existing release context...");
    let changelog_entry = read_changelog_entry(&ctx.repo_root, &ctx.tag);
    let existing_release = if let Some(gh) = &ctx.github_client {
        match gh.get_release_by_tag(&ctx.tag).await? {
            Some(r) => r.body,
            None => None,
        }
    } else {
        None
    };

    let emoji = ctx.defaults.emoji.unwrap_or(true);
    let system = prompt::system_prompt(ctx.system_extra.as_deref(), emoji);
    let user_msg = prompt::user_prompt(&prompt::UserPromptContext {
        tag: &ctx.tag,
        prev_tag: &ctx.prev_tag,
        owner_repo: &ctx.owner_repo,
        git_log: &git_log,
        pr_numbers: &pr_numbers,
        changelog_entry: changelog_entry.as_deref(),
        existing_release: existing_release.as_deref(),
        context: ctx.context.as_deref(),
    });

    job.prop("message", "Generating release notes...");
    let anthropic =
        anthropic::AnthropicClient::new(ctx.api_key.clone(), ctx.model.clone(), ctx.max_tokens);
    let tool_defs = tools::all_definitions(ctx.github_client.is_some());

    let verify_links = !dry_run && ctx.defaults.verify_links.unwrap_or(true);

    agent::run(agent::AgentContext {
        client: &anthropic,
        system: &system,
        user_message: &user_msg,
        tool_defs,
        repo_root: &ctx.repo_root,
        github: ctx.github_client.as_ref(),
        verify_links,
        job,
    })
    .await
    .map_err(Into::into)
}

async fn publish(
    opts: &GenerateOptions,
    ctx: &Context,
    parsed: &ParsedOutput,
    job: &Arc<ProgressJob>,
) -> miette::Result<()> {
    if opts.github_release && !opts.dry_run {
        job.prop(
            "message",
            &format!("Updating GitHub release for {}...", ctx.tag),
        );
        let gh = ctx.github_client.as_ref().unwrap();
        match gh.get_release_by_tag(&ctx.tag).await? {
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
                    &format!("No GitHub release found for {} — skipping update", ctx.tag),
                );
            }
        }
    }

    Ok(())
}

fn read_changelog_entry(repo_root: &Path, tag: &str) -> Option<String> {
    let path = repo_root.join("CHANGELOG.md");
    let contents = xx::file::read_to_string(&path).ok()?;

    let version = tag.strip_prefix('v').unwrap_or(tag);
    let header_pattern = format!("## [{version}]");
    let alt_pattern = format!("## {version}");

    let start = contents
        .find(&header_pattern)
        .or_else(|| contents.find(&alt_pattern))?;

    let rest = &contents[start..];
    let end = rest[3..]
        .find("\n## ")
        .map(|i| start + 3 + i)
        .unwrap_or(contents.len());

    Some(contents[start..end].trim().to_string())
}
