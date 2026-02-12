use std::path::{Path, PathBuf};
use std::sync::Arc;

use clx::progress::{ProgressJob, ProgressJobBuilder, ProgressStatus};
use log::info;

use crate::config::Defaults;
use crate::llm::LlmClient;
use crate::output::ParsedOutput;
use crate::providers::{self, Provider};
use crate::{agent, config, git, github, prompt, tools};

pub struct GenerateOptions {
    pub tag: String,
    pub prev_tag: Option<String>,
    pub github_release: bool,
    pub concise: bool,
    pub dry_run: bool,
    pub repo: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub provider: Option<Provider>,
    pub base_url: Option<String>,
    pub output: Option<PathBuf>,
}

struct Context {
    repo_root: PathBuf,
    #[allow(dead_code)]
    owner_repo: String,
    tag: String,
    prev_tag: String,
    client: Box<dyn LlmClient>,
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

    let text = if opts.concise {
        parsed.changelog.clone()
    } else {
        format!("# {}\n\n{}", parsed.release_title, parsed.release_body)
    };

    if let Some(path) = &opts.output {
        xx::file::write(path, &text)?;
    } else {
        println!("{text}");
    }

    Ok(())
}

async fn gather_context(opts: &GenerateOptions, job: &Arc<ProgressJob>) -> miette::Result<Context> {
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

    // Determine provider
    let provider = opts
        .provider
        .clone()
        .or(defaults.provider.clone())
        .unwrap_or_else(|| providers::detect_provider(&model));
    info!("provider: {provider:?}, model: {model}");

    // Resolve API key based on provider
    let api_key = match &provider {
        Provider::Anthropic => std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| crate::error::Error::Llm("ANTHROPIC_API_KEY not set".into()))?,
        Provider::OpenAI => std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("LLM_API_KEY"))
            .unwrap_or_default(),
    };

    let base_url = opts
        .base_url
        .clone()
        .or(defaults.base_url.clone())
        .and_then(|u| if u.is_empty() { None } else { Some(u) });

    let client = providers::build_client(&provider, api_key, model, max_tokens, base_url);

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
        client,
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

    // Fetch existing release and recent releases in parallel
    let match_style = ctx.defaults.match_style.unwrap_or(true);
    let (existing_release, recent_releases) = if let Some(gh) = &ctx.github_client {
        let existing_fut = gh.get_release_by_tag(&ctx.tag);
        let recent_fut = async {
            if !match_style {
                return vec![];
            }
            match gh.list_recent_releases(3).await {
                Ok(releases) => releases
                    .into_iter()
                    .filter(|r| r.tag_name != ctx.tag)
                    .filter_map(|r| {
                        let body = r.body.unwrap_or_default();
                        if body.is_empty() {
                            None
                        } else {
                            Some((r.tag_name, body))
                        }
                    })
                    .take(2)
                    .collect(),
                Err(e) => {
                    info!("failed to fetch recent releases for style matching: {e}");
                    vec![]
                }
            }
        };
        let (existing_result, recent) = tokio::join!(existing_fut, recent_fut);
        let existing = match existing_result? {
            Some(r) => r.body,
            None => None,
        };
        (existing, recent)
    } else {
        (None, vec![])
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
        recent_releases: &recent_releases,
    });

    job.prop("message", "Generating release notes...");
    let tool_defs = tools::all_definitions(ctx.github_client.is_some());

    let verify_links = !dry_run && ctx.defaults.verify_links.unwrap_or(true);

    agent::run(agent::AgentContext {
        client: &*ctx.client,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_changelog_entry_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CHANGELOG.md"),
            "## [1.0.0]\n### Added\n- Feature\n\n## [0.9.0]\n### Fixed\n- Bug\n",
        )
        .unwrap();
        let entry = read_changelog_entry(dir.path(), "v1.0.0").unwrap();
        assert!(entry.contains("### Added"));
        assert!(entry.contains("Feature"));
        assert!(!entry.contains("0.9.0"));
    }

    #[test]
    fn test_read_changelog_entry_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CHANGELOG.md"), "## [0.9.0]\n- old\n").unwrap();
        let entry = read_changelog_entry(dir.path(), "v2.0.0");
        assert!(entry.is_none());
    }

    #[test]
    fn test_read_changelog_entry_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let entry = read_changelog_entry(dir.path(), "v1.0.0");
        assert!(entry.is_none());
    }

    #[test]
    fn test_read_changelog_entry_alt_format() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CHANGELOG.md"),
            "## 1.0.0\n### Changed\n- Something\n",
        )
        .unwrap();
        let entry = read_changelog_entry(dir.path(), "v1.0.0").unwrap();
        assert!(entry.contains("### Changed"));
    }
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
