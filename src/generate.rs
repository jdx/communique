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
    pub changelog: bool,
    pub concise: bool,
    pub dry_run: bool,
    pub repo: Option<String>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub provider: Option<Provider>,
    pub base_url: Option<String>,
    pub output: Option<PathBuf>,
    pub config: Option<PathBuf>,
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

impl Context {
    fn is_unreleased_head(&self) -> bool {
        self.tag == "HEAD"
    }

    fn display_tag(&self) -> &str {
        if self.is_unreleased_head() {
            "Unreleased"
        } else {
            &self.tag
        }
    }
}

fn validate_generate_options(opts: &GenerateOptions) -> miette::Result<()> {
    if opts.github_release && opts.tag == "HEAD" {
        return Err(miette::miette!(
            "--github-release cannot be used with HEAD because HEAD is an unreleased changelog target. Use --changelog, or generate notes for a real tag."
        ));
    }

    Ok(())
}

fn release_title_description<'a>(title: &'a str, label: &str) -> Option<&'a str> {
    let rest = title.strip_prefix(label)?;
    if rest.chars().next().is_some_and(char::is_alphanumeric) {
        return None;
    }

    let trimmed = rest.trim_start_matches(|c: char| !c.is_alphanumeric());
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn normalize_release_title(title: String, display_tag: &str, raw_tag: &str) -> String {
    let tag_prefix = format!("{display_tag}: ");
    if title.starts_with(&tag_prefix) {
        return title;
    }

    if raw_tag != display_tag && title.trim() == raw_tag {
        return format!("{display_tag}: changes");
    }

    let description = release_title_description(&title, display_tag)
        .or_else(|| {
            if raw_tag == display_tag {
                None
            } else {
                release_title_description(&title, raw_tag)
            }
        })
        .unwrap_or(title.as_str());

    format!("{display_tag}: {description}")
}

pub async fn run(opts: GenerateOptions) -> miette::Result<()> {
    let job = ProgressJobBuilder::new()
        .body("{{spinner()}} {{message | flex}}")
        .prop("message", "Discovering repository...")
        .start();

    let ctx = gather_context(&opts, &job).await?;
    let mut parsed = generate_notes(&ctx, opts.dry_run, &job).await?;

    // Normalize release title to "label: description" format.
    // The LLM may include the tag with a different separator (e.g. "v1.0.0 (title)"
    // or "v1.0.0 - title"), so we check for the exact "label: " prefix.
    let display_tag = ctx.display_tag();
    parsed.release_title = normalize_release_title(parsed.release_title, display_tag, &ctx.tag);

    publish(&opts, &ctx, &parsed, &job).await?;

    if opts.changelog {
        update_changelog(&ctx, &parsed, opts.dry_run, &job).await?;
    }

    job.set_status(ProgressStatus::Done);
    job.prop("message", "Done");
    clx::progress::flush();

    let u = &parsed.usage;
    eprintln!(
        "Tokens: {} input + {} output = {} total",
        u.input_tokens,
        u.output_tokens,
        u.input_tokens + u.output_tokens
    );

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
    validate_generate_options(opts)?;

    let github_token = std::env::var("GITHUB_TOKEN").ok();

    if opts.github_release && github_token.is_none() {
        return Err(crate::error::Error::GitHub(
            "GITHUB_TOKEN is required for --github-release".into(),
        ))?;
    }

    let repo_root = git::repo_root()?;
    info!("repo root: {}", repo_root.display());

    let config = match &opts.config {
        Some(path) => config::Config::load_from(path)?,
        None => config::Config::load(&repo_root)?,
    }
    .unwrap_or_default();
    let defaults = config.defaults.unwrap_or_default();

    let model = opts
        .model
        .clone()
        .or(defaults.model.clone())
        .unwrap_or_else(|| "claude-opus-4-7".into());
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
    let changelog_entry = if ctx.is_unreleased_head() {
        read_unreleased_section(&ctx.repo_root)?
    } else {
        read_changelog_entry(&ctx.repo_root, &ctx.tag)
    };

    // Fetch existing release and recent releases in parallel
    let match_style = ctx.defaults.match_style.unwrap_or(true);
    let (existing_release, recent_releases) = if let Some(gh) = &ctx.github_client {
        let existing_fut = async {
            if ctx.is_unreleased_head() {
                Ok(None)
            } else {
                gh.get_release_by_tag(&ctx.tag).await
            }
        };
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
        is_unreleased_head: ctx.is_unreleased_head(),
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
    use crate::llm::Usage;
    use crate::llm::{StopReason, ToolCall, TurnResponse};
    use crate::test_helpers::{
        MockLlmClient, TempRepo, fake_usage, fake_usage_with, submit_tool_call,
    };
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_opts(tag: &str) -> GenerateOptions {
        GenerateOptions {
            tag: tag.into(),
            prev_tag: None,
            github_release: false,
            changelog: false,
            concise: false,
            dry_run: false,
            repo: None,
            model: None,
            max_tokens: None,
            provider: None,
            base_url: None,
            output: None,
            config: None,
        }
    }

    fn test_context(repo_root: PathBuf, tag: &str, prev_tag: &str) -> Context {
        Context {
            repo_root,
            owner_repo: "test/repo".into(),
            tag: tag.into(),
            prev_tag: prev_tag.into(),
            client: Box::new(MockLlmClient::new(vec![])),
            defaults: Defaults::default(),
            system_extra: None,
            context: None,
            github_client: None,
        }
    }

    fn test_parsed_output(changelog: &str) -> ParsedOutput {
        ParsedOutput {
            changelog: changelog.into(),
            release_title: "Title".into(),
            release_body: "Body".into(),
            usage: Usage::default(),
        }
    }

    #[test]
    fn test_validate_generate_options_rejects_head_github_release() {
        let opts = GenerateOptions {
            github_release: true,
            ..test_opts("HEAD")
        };

        let err = validate_generate_options(&opts).unwrap_err().to_string();
        assert!(err.contains(
            "--github-release cannot be used with HEAD because HEAD is an unreleased changelog target. Use --changelog, or generate notes for a real tag."
        ));
    }

    #[test]
    fn test_validate_generate_options_allows_head_changelog() {
        let opts = GenerateOptions {
            changelog: true,
            ..test_opts("HEAD")
        };

        validate_generate_options(&opts).unwrap();
    }

    #[test]
    fn test_normalize_release_title_uses_unreleased_display_tag_for_head() {
        let title = normalize_release_title("HEAD - Draft changes".into(), "Unreleased", "HEAD");
        assert_eq!(title, "Unreleased: Draft changes");
        assert!(!title.starts_with("HEAD:"));
    }

    #[test]
    fn test_normalize_release_title_preserves_tagged_behavior() {
        let title = normalize_release_title("v1.2.3 - Feature release".into(), "v1.2.3", "v1.2.3");
        assert_eq!(title, "v1.2.3: Feature release");
    }

    #[test]
    fn test_normalize_release_title_does_not_strip_head_inside_word() {
        let title = normalize_release_title("HEADline feature".into(), "Unreleased", "HEAD");
        assert_eq!(title, "Unreleased: HEADline feature");
    }

    #[tokio::test]
    async fn test_generate_notes_basic() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# Hello");
        repo.commit("initial commit");
        repo.tag("v0.9.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: add main");
        repo.tag("v1.0.0");

        let mock_client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![submit_tool_call(
                "### Added\n- Main function",
                "Initial Release",
                "First release.",
            )],
            text: None,
            stop_reason: StopReason::ToolUse,
            usage: fake_usage(),
        }]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults {
                verify_links: Some(false),
                ..Defaults::default()
            },
            system_extra: None,
            context: None,
            github_client: None,
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        let parsed = generate_notes(&ctx, false, &job).await.unwrap();
        assert_eq!(parsed.release_title, "Initial Release");
        assert!(parsed.changelog.contains("Main function"));
    }

    #[tokio::test]
    async fn test_generate_notes_with_github() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# Hello");
        repo.commit("initial");
        repo.tag("v0.9.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: add feature (#1)");
        repo.tag("v1.0.0");

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases/tags/v1.0.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 1, "tag_name": "v1.0.0", "name": "v1.0.0",
                "body": "Existing notes"
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"id": 2, "tag_name": "v0.9.0", "name": "v0.9.0", "body": "Previous notes"}
            ])))
            .mount(&server)
            .await;

        let gh =
            github::GitHubClient::with_base_url("test-token".into(), "test/repo", server.uri())
                .unwrap();

        let mock_client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![submit_tool_call("### Added\n- Feature", "Title", "Body")],
            text: None,
            stop_reason: StopReason::ToolUse,
            usage: fake_usage(),
        }]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults {
                verify_links: Some(false),
                ..Defaults::default()
            },
            system_extra: Some("Extra instructions".into()),
            context: Some("Test project".into()),
            github_client: Some(gh),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        let parsed = generate_notes(&ctx, false, &job).await.unwrap();
        assert_eq!(parsed.release_title, "Title");
        assert_eq!(parsed.release_body, "Body");
    }

    #[tokio::test]
    async fn test_publish_updates_release() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases/tags/v1.0.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 42, "tag_name": "v1.0.0", "name": "v1.0.0", "body": "old"
            })))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/releases/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 42})))
            .expect(1)
            .mount(&server)
            .await;

        let gh =
            github::GitHubClient::with_base_url("test-token".into(), "test/repo", server.uri())
                .unwrap();

        let ctx = Context {
            repo_root: PathBuf::from("/tmp"),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(MockLlmClient::new(vec![])),
            defaults: Defaults::default(),
            system_extra: None,
            context: None,
            github_client: Some(gh),
        };

        let opts = GenerateOptions {
            github_release: true,
            dry_run: false,
            ..test_opts("v1.0.0")
        };

        let parsed = ParsedOutput {
            changelog: "changes".into(),
            release_title: "Title".into(),
            release_body: "Body".into(),
            usage: Usage::default(),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        publish(&opts, &ctx, &parsed, &job).await.unwrap();
        // wiremock expect(1) verifies PATCH was called
    }

    #[tokio::test]
    async fn test_publish_dry_run_skips_update() {
        let ctx = Context {
            repo_root: PathBuf::from("/tmp"),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(MockLlmClient::new(vec![])),
            defaults: Defaults::default(),
            system_extra: None,
            context: None,
            github_client: None,
        };

        let opts = GenerateOptions {
            github_release: true,
            dry_run: true,
            ..test_opts("v1.0.0")
        };

        let parsed = ParsedOutput {
            changelog: "changes".into(),
            release_title: "Title".into(),
            release_body: "Body".into(),
            usage: Usage::default(),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        publish(&opts, &ctx, &parsed, &job).await.unwrap();
    }

    #[tokio::test]
    async fn test_publish_no_release_found() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases/tags/v1.0.0"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        // Fallback list endpoint also returns nothing matching
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;

        let gh =
            github::GitHubClient::with_base_url("test-token".into(), "test/repo", server.uri())
                .unwrap();

        let ctx = Context {
            repo_root: PathBuf::from("/tmp"),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(MockLlmClient::new(vec![])),
            defaults: Defaults::default(),
            system_extra: None,
            context: None,
            github_client: Some(gh),
        };

        let opts = GenerateOptions {
            github_release: true,
            dry_run: false,
            ..test_opts("v1.0.0")
        };

        let parsed = ParsedOutput {
            changelog: "changes".into(),
            release_title: "Title".into(),
            release_body: "Body".into(),
            usage: Usage::default(),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        // Should not error — just warns and skips
        publish(&opts, &ctx, &parsed, &job).await.unwrap();
    }

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

    #[test]
    fn test_read_unreleased_section_bracketed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CHANGELOG.md"),
            "# Changelog\n\n## [Unreleased]\n\n### Changed\n- Draft\n\n## [1.0.0]\n- Old\n",
        )
        .unwrap();

        let entry = read_unreleased_section(dir.path()).unwrap().unwrap();
        assert_eq!(entry, "### Changed\n- Draft");
        assert!(!entry.contains("1.0.0"));
    }

    #[test]
    fn test_read_unreleased_section_plain() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("CHANGELOG.md"),
            "# Changelog\n\n## Unreleased\n\n### Fixed\n- Draft bug\n\n## 1.0.0\n- Old\n",
        )
        .unwrap();

        let entry = read_unreleased_section(dir.path()).unwrap().unwrap();
        assert_eq!(entry, "### Fixed\n- Draft bug");
    }

    #[test]
    fn test_read_unreleased_section_missing_file_or_section() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_unreleased_section(dir.path()).unwrap().is_none());

        std::fs::write(
            dir.path().join("CHANGELOG.md"),
            "# Changelog\n\n## [1.0.0]\n- Old\n",
        )
        .unwrap();
        assert!(read_unreleased_section(dir.path()).unwrap().is_none());
    }

    #[tokio::test]
    async fn test_e2e_tool_use_then_submit() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# My Project\nA cool tool.");
        repo.commit("initial commit");
        repo.tag("v0.9.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: add main");
        repo.tag("v1.0.0");

        let mock_client = MockLlmClient::new(vec![
            // Turn 1: LLM asks to read a file — dispatched against real repo
            TurnResponse {
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "README.md"}),
                }],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            // Turn 2: LLM submits release notes
            TurnResponse {
                tool_calls: vec![submit_tool_call(
                    "### Added\n- Main function",
                    "v1.0.0",
                    "First release with main entry point.",
                )],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
        ]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults {
                verify_links: Some(false),
                ..Defaults::default()
            },
            system_extra: None,
            context: None,
            github_client: None,
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        let parsed = generate_notes(&ctx, false, &job).await.unwrap();
        assert_eq!(parsed.release_title, "v1.0.0");
        assert!(parsed.release_body.contains("main entry point"));
    }

    #[tokio::test]
    async fn test_e2e_multiple_tools_then_submit() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# Project");
        repo.write_file("src/main.rs", "fn main() { println!(\"hello\"); }");
        repo.commit("initial");
        repo.tag("v0.9.0");
        repo.write_file("src/lib.rs", "pub fn greet() {}");
        repo.commit("feat: add greeting lib");
        repo.tag("v1.0.0");

        let mock_client = MockLlmClient::new(vec![
            // Turn 1: list_files + grep (concurrent tool dispatch)
            TurnResponse {
                tool_calls: vec![
                    ToolCall {
                        id: "call_1".into(),
                        name: "list_files".into(),
                        input: json!({}),
                    },
                    ToolCall {
                        id: "call_2".into(),
                        name: "grep".into(),
                        input: json!({"pattern": "fn main"}),
                    },
                ],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            // Turn 2: git_show + get_commits (concurrent tool dispatch)
            TurnResponse {
                tool_calls: vec![
                    ToolCall {
                        id: "call_3".into(),
                        name: "git_show".into(),
                        input: json!({"ref": "HEAD"}),
                    },
                    ToolCall {
                        id: "call_4".into(),
                        name: "get_commits".into(),
                        input: json!({}),
                    },
                ],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            // Turn 3: submit
            TurnResponse {
                tool_calls: vec![submit_tool_call(
                    "### Added\n- Greeting library",
                    "v1.0.0",
                    "Added greeting functionality.",
                )],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
        ]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults {
                verify_links: Some(false),
                ..Defaults::default()
            },
            system_extra: None,
            context: None,
            github_client: None,
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        let parsed = generate_notes(&ctx, false, &job).await.unwrap();
        assert_eq!(parsed.release_title, "v1.0.0");
        assert!(parsed.release_body.contains("greeting"));
    }

    #[tokio::test]
    async fn test_e2e_with_github() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# Hello");
        repo.commit("initial");
        repo.tag("v0.9.0");
        repo.write_file("src/feature.rs", "pub fn feature() {}");
        repo.commit("feat: add feature (#1)");
        repo.tag("v1.0.0");

        let server = MockServer::start().await;

        // Release lookup
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases/tags/v1.0.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 1, "tag_name": "v1.0.0", "name": "v1.0.0",
                "body": "Existing notes"
            })))
            .mount(&server)
            .await;

        // Release list for style matching
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"id": 2, "tag_name": "v0.9.0", "name": "v0.9.0", "body": "Previous notes"}
            ])))
            .mount(&server)
            .await;

        // PR details (get_pr tool)
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/1"))
            .and(header("Accept", "application/vnd.github+json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "number": 1,
                "title": "Add feature",
                "body": "Adds a new feature",
                "user": {"login": "dev"},
                "labels": [{"name": "enhancement"}]
            })))
            .mount(&server)
            .await;

        // PR diff (get_pr_diff tool)
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/1"))
            .and(header("Accept", "application/vnd.github.v3.diff"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "diff --git a/src/feature.rs b/src/feature.rs\n+pub fn feature() {}",
            ))
            .mount(&server)
            .await;

        // Issue details (get_issue tool)
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "number": 1,
                "title": "Feature request",
                "body": "Please add this feature",
                "state": "closed",
                "user": {"login": "requester"},
                "labels": [{"name": "enhancement"}]
            })))
            .mount(&server)
            .await;

        let gh =
            github::GitHubClient::with_base_url("test-token".into(), "test/repo", server.uri())
                .unwrap();

        let mock_client = MockLlmClient::new(vec![
            // Turn 1: get_pr + get_pr_diff (concurrent)
            TurnResponse {
                tool_calls: vec![
                    ToolCall {
                        id: "call_1".into(),
                        name: "get_pr".into(),
                        input: json!({"number": 1}),
                    },
                    ToolCall {
                        id: "call_2".into(),
                        name: "get_pr_diff".into(),
                        input: json!({"number": 1}),
                    },
                ],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            // Turn 2: get_issue
            TurnResponse {
                tool_calls: vec![ToolCall {
                    id: "call_3".into(),
                    name: "get_issue".into(),
                    input: json!({"number": 1}),
                }],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            // Turn 3: submit
            TurnResponse {
                tool_calls: vec![submit_tool_call(
                    "### Added\n- New feature (#1)",
                    "v1.0.0 - Feature Release",
                    "Added feature based on #1.",
                )],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
        ]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults {
                verify_links: Some(false),
                ..Defaults::default()
            },
            system_extra: None,
            context: None,
            github_client: Some(gh),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        let parsed = generate_notes(&ctx, false, &job).await.unwrap();
        assert_eq!(parsed.release_title, "v1.0.0 - Feature Release");
        assert!(parsed.changelog.contains("feature"));
    }

    #[tokio::test]
    async fn test_e2e_publish_github_release() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# Project");
        repo.commit("initial");
        repo.tag("v0.9.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: initial release");
        repo.tag("v1.0.0");

        let server = MockServer::start().await;

        // Release lookup (used by both generate_notes and publish)
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases/tags/v1.0.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 42, "tag_name": "v1.0.0", "name": "v1.0.0",
                "body": "Draft release"
            })))
            .mount(&server)
            .await;

        // Release list for style matching
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;

        // PATCH to update release — expect(1) verifies it's called
        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/releases/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 42})))
            .expect(1)
            .mount(&server)
            .await;

        let gh =
            github::GitHubClient::with_base_url("test-token".into(), "test/repo", server.uri())
                .unwrap();

        let mock_client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![submit_tool_call(
                "### Added\n- Initial release",
                "v1.0.0",
                "First release.",
            )],
            text: None,
            stop_reason: StopReason::ToolUse,
            usage: fake_usage(),
        }]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults {
                verify_links: Some(false),
                ..Defaults::default()
            },
            system_extra: None,
            context: None,
            github_client: Some(gh),
        };

        let opts = GenerateOptions {
            github_release: true,
            dry_run: false,
            ..test_opts("v1.0.0")
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        let parsed = generate_notes(&ctx, false, &job).await.unwrap();
        publish(&opts, &ctx, &parsed, &job).await.unwrap();
        // wiremock expect(1) verifies PATCH was called
    }

    #[tokio::test]
    async fn test_e2e_usage_accumulation() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# Hello");
        repo.commit("initial");
        repo.tag("v0.9.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: add main");
        repo.tag("v1.0.0");

        let mock_client = MockLlmClient::new(vec![
            // Turn 1: read a file with non-zero usage
            TurnResponse {
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "read_file".into(),
                    input: json!({"path": "README.md"}),
                }],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage_with(100, 50),
            },
            // Turn 2: submit with non-zero usage
            TurnResponse {
                tool_calls: vec![submit_tool_call(
                    "### Added\n- Main function",
                    "v1.0.0",
                    "Release notes.",
                )],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage_with(150, 75),
            },
        ]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults {
                verify_links: Some(false),
                ..Defaults::default()
            },
            system_extra: None,
            context: None,
            github_client: None,
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        let parsed = generate_notes(&ctx, false, &job).await.unwrap();
        assert_eq!(parsed.usage.input_tokens, 250);
        assert_eq!(parsed.usage.output_tokens, 125);
    }

    #[test]
    fn test_split_changelog_small() {
        let content =
            "# Changelog\n\n## [Unreleased]\n\n## [1.0.0] - 2025-01-01\n### Added\n- Feature\n";
        let (head, tail) = split_changelog(content, 3);
        assert_eq!(head, content);
        assert_eq!(tail, "");
    }

    #[test]
    fn test_split_changelog_large() {
        let content = "\
# Changelog

## [Unreleased]

## [3.0.0] - 2025-03-01
### Added
- Three

## [2.0.0] - 2025-02-01
### Added
- Two

## [1.0.0] - 2025-01-01
### Added
- One

## [0.9.0] - 2024-12-01
### Fixed
- Zero nine
";
        let (head, tail) = split_changelog(content, 3);
        assert!(head.contains("[3.0.0]"));
        assert!(head.contains("[2.0.0]"));
        assert!(head.contains("[1.0.0]"));
        assert!(!head.contains("[0.9.0]"));
        assert!(tail.contains("[0.9.0]"));
    }

    #[test]
    fn test_replace_unreleased_section_bracketed_preserves_tail() {
        let existing = "# Changelog\n\n## [Unreleased]\n\n### Changed\n- Old draft\n\n## [1.0.0] - 2025-01-01\n### Added\n- Old release\n";
        let updated = replace_unreleased_section(existing, "### Added\n- New draft").unwrap();

        assert_eq!(
            updated,
            "# Changelog\n\n## [Unreleased]\n\n### Added\n- New draft\n\n## [1.0.0] - 2025-01-01\n### Added\n- Old release\n"
        );
    }

    #[test]
    fn test_replace_unreleased_section_plain_preserves_header() {
        let existing = "# Changelog\n\n## Unreleased\n\n- Old draft\n";
        let updated = replace_unreleased_section(existing, "### Fixed\n- New fix").unwrap();

        assert_eq!(
            updated,
            "# Changelog\n\n## Unreleased\n\n### Fixed\n- New fix\n"
        );
    }

    #[test]
    fn test_replace_unreleased_section_errors_without_unreleased_header() {
        let err = replace_unreleased_section("# Changelog\n\n## [1.0.0]\n- Old\n", "- New")
            .unwrap_err()
            .to_string();

        assert!(err.contains("does not contain ## [Unreleased] or ## Unreleased"));
    }

    #[test]
    fn test_replace_unreleased_section_errors_on_duplicate_unreleased_headers() {
        let err = replace_unreleased_section(
            "# Changelog\n\n## [Unreleased]\n\n## Unreleased\n",
            "- New",
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("multiple Unreleased sections"));
    }

    #[test]
    fn test_replace_unreleased_section_removes_forbidden_target_headers() {
        let existing = "# Changelog\n\n## [Unreleased]\n\n- Old\n";
        let updated = replace_unreleased_section(existing, "## [HEAD]\n### Added\n- New").unwrap();

        assert!(!updated.contains("## [HEAD]"));
        assert!(!updated.contains("## HEAD"));
        assert!(updated.contains("### Added\n- New"));
    }

    #[tokio::test]
    async fn test_update_changelog_head_updates_unreleased_in_place() {
        let repo = TempRepo::new();
        let original = "# Changelog\n\nIntro text.\n\n## [Unreleased]\n\n### Changed\n- Old draft\n\n## [0.1.0] - 2026-01-01\n### Added\n- Initial release\n";
        repo.write_file("CHANGELOG.md", original);

        let ctx = test_context(repo.path().to_path_buf(), "HEAD", "v0.1.0");
        let parsed = test_parsed_output("### Added\n- New draft");
        let job = Arc::new(ProgressJobBuilder::new().build());

        update_changelog(&ctx, &parsed, false, &job).await.unwrap();

        let result = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        assert!(result.contains("Intro text."));
        assert!(result.contains("## [Unreleased]\n\n### Added\n- New draft"));
        assert!(result.contains("## [0.1.0] - 2026-01-01\n### Added\n- Initial release"));
        assert!(!result.contains("## [HEAD]"));
        assert!(!result.contains("## HEAD"));
    }

    #[tokio::test]
    async fn test_update_changelog_head_creates_missing_file() {
        let repo = TempRepo::new();
        let ctx = test_context(repo.path().to_path_buf(), "HEAD", "v0.1.0");
        let parsed = test_parsed_output("### Added\n- New draft");
        let job = Arc::new(ProgressJobBuilder::new().build());

        update_changelog(&ctx, &parsed, false, &job).await.unwrap();

        let result = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        assert_eq!(
            result,
            "# Changelog\n\n## [Unreleased]\n\n### Added\n- New draft\n"
        );
    }

    #[tokio::test]
    async fn test_update_changelog_head_errors_without_unreleased_header() {
        let repo = TempRepo::new();
        repo.write_file("CHANGELOG.md", "# Changelog\n\n## [0.1.0]\n- Old\n");
        let ctx = test_context(repo.path().to_path_buf(), "HEAD", "v0.1.0");
        let parsed = test_parsed_output("### Added\n- New draft");
        let job = Arc::new(ProgressJobBuilder::new().build());

        let err = update_changelog(&ctx, &parsed, false, &job)
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("does not contain ## [Unreleased] or ## Unreleased"));
    }

    #[tokio::test]
    async fn test_update_changelog_head_dry_run_leaves_file_unchanged() {
        let repo = TempRepo::new();
        let original = "# Changelog\n\n## [Unreleased]\n\n### Changed\n- Old draft\n";
        repo.write_file("CHANGELOG.md", original);
        let ctx = test_context(repo.path().to_path_buf(), "HEAD", "v0.1.0");
        let parsed = test_parsed_output("### Added\n- New draft");
        let job = Arc::new(ProgressJobBuilder::new().build());

        update_changelog(&ctx, &parsed, true, &job).await.unwrap();

        let result = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        assert_eq!(result, original);
    }

    #[tokio::test]
    async fn test_update_changelog_insert() {
        let repo = TempRepo::new();
        repo.write_file(
            "CHANGELOG.md",
            "# Changelog\n\n## [Unreleased]\n\n## [0.9.0] - 2025-01-01\n### Fixed\n- Bug\n",
        );
        repo.commit("initial");
        repo.tag("v0.9.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: add main");
        repo.tag("v1.0.0");

        let updated_head = "\
# Changelog

## [Unreleased]

## [1.0.0] - 2025-02-12
### Added
- Main function

## [0.9.0] - 2025-01-01
### Fixed
- Bug
";

        let mock_client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![],
            text: Some(updated_head.to_string()),
            stop_reason: StopReason::EndTurn,
            usage: fake_usage_with(200, 100),
        }]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults::default(),
            system_extra: None,
            context: None,
            github_client: None,
        };

        let parsed = ParsedOutput {
            changelog: "### Added\n- Main function".into(),
            release_title: "v1.0.0".into(),
            release_body: "Release notes.".into(),
            usage: Usage::default(),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        update_changelog(&ctx, &parsed, false, &job).await.unwrap();

        let result = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        assert!(result.contains("[1.0.0]"));
        assert!(result.contains("Main function"));
        assert!(result.contains("[0.9.0]"));
    }

    #[tokio::test]
    async fn test_update_changelog_new_file() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# Hello");
        repo.commit("initial");
        repo.tag("v0.9.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: add main");
        repo.tag("v1.0.0");

        // No CHANGELOG.md exists
        let updated = "\
# Changelog

## [Unreleased]

## [1.0.0] - 2025-02-12
### Added
- Main function
";

        let mock_client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![],
            text: Some(updated.to_string()),
            stop_reason: StopReason::EndTurn,
            usage: fake_usage(),
        }]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults::default(),
            system_extra: None,
            context: None,
            github_client: None,
        };

        let parsed = ParsedOutput {
            changelog: "### Added\n- Main function".into(),
            release_title: "v1.0.0".into(),
            release_body: "Body.".into(),
            usage: Usage::default(),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        update_changelog(&ctx, &parsed, false, &job).await.unwrap();

        let result = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        assert!(result.contains("[1.0.0]"));
        assert!(result.contains("Main function"));
    }

    #[tokio::test]
    async fn test_update_changelog_dry_run() {
        let repo = TempRepo::new();
        let original = "# Changelog\n\n## [Unreleased]\n";
        repo.write_file("CHANGELOG.md", original);
        repo.commit("initial");
        repo.tag("v0.9.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: add main");
        repo.tag("v1.0.0");

        let mock_client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![],
            text: Some("# Changelog\n\n## [Unreleased]\n\n## [1.0.0]\n### Added\n- Stuff\n".into()),
            stop_reason: StopReason::EndTurn,
            usage: fake_usage(),
        }]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v1.0.0".into(),
            prev_tag: "v0.9.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults::default(),
            system_extra: None,
            context: None,
            github_client: None,
        };

        let parsed = ParsedOutput {
            changelog: "### Added\n- Stuff".into(),
            release_title: "v1.0.0".into(),
            release_body: "Body.".into(),
            usage: Usage::default(),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        update_changelog(&ctx, &parsed, true, &job).await.unwrap();

        // File should be unchanged
        let result = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        assert_eq!(result, original);
    }

    #[tokio::test]
    async fn test_update_changelog_with_tail() {
        let repo = TempRepo::new();
        let content = "\
# Changelog

## [Unreleased]

## [3.0.0] - 2025-03-01
### Added
- Three

## [2.0.0] - 2025-02-01
### Added
- Two

## [1.0.0] - 2025-01-01
### Added
- One

## [0.9.0] - 2024-12-01
### Fixed
- Zero nine
";
        repo.write_file("CHANGELOG.md", content);
        repo.commit("initial");
        repo.tag("v3.0.0");
        repo.write_file("src/main.rs", "fn main() {}");
        repo.commit("feat: new feature");
        repo.tag("v4.0.0");

        // LLM returns updated head with new version inserted
        let updated_head = "\
# Changelog

## [Unreleased]

## [4.0.0] - 2025-04-01
### Added
- New feature

## [3.0.0] - 2025-03-01
### Added
- Three

## [2.0.0] - 2025-02-01
### Added
- Two

## [1.0.0] - 2025-01-01
### Added
- One
";

        let mock_client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![],
            text: Some(updated_head.to_string()),
            stop_reason: StopReason::EndTurn,
            usage: fake_usage(),
        }]);

        let ctx = Context {
            repo_root: repo.path().to_path_buf(),
            owner_repo: "test/repo".into(),
            tag: "v4.0.0".into(),
            prev_tag: "v3.0.0".into(),
            client: Box::new(mock_client),
            defaults: Defaults::default(),
            system_extra: None,
            context: None,
            github_client: None,
        };

        let parsed = ParsedOutput {
            changelog: "### Added\n- New feature".into(),
            release_title: "v4.0.0".into(),
            release_body: "Body.".into(),
            usage: Usage::default(),
        };

        let job = Arc::new(ProgressJobBuilder::new().build());
        update_changelog(&ctx, &parsed, false, &job).await.unwrap();

        let result = std::fs::read_to_string(repo.path().join("CHANGELOG.md")).unwrap();
        // Should contain the new version from LLM
        assert!(result.contains("[4.0.0]"));
        assert!(result.contains("New feature"));
        // Should preserve the tail (0.9.0)
        assert!(result.contains("[0.9.0]"));
        assert!(result.contains("Zero nine"));
    }

    #[test]
    fn test_today_iso() {
        let date = today_iso();
        // Should be YYYY-MM-DD format
        assert_eq!(date.len(), 10);
        assert_eq!(date.as_bytes()[4], b'-');
        assert_eq!(date.as_bytes()[7], b'-');
    }
}

/// Split changelog content into (head, tail) keeping at most `max_versions` versioned sections
/// in the head. This limits tokens sent to the LLM for large changelogs.
fn split_changelog(content: &str, max_versions: usize) -> (&str, &str) {
    let mut version_count = 0;
    let mut search_start = 0;

    loop {
        let rest = &content[search_start..];
        let Some(pos) = rest.find("\n## ") else {
            break;
        };
        let abs_pos = search_start + pos;
        let header_start = abs_pos + 1; // skip the \n

        // Check if this is [Unreleased] — don't count it
        let header_line = &content[header_start..];
        let is_unreleased = header_line.strip_prefix("## ").is_some_and(|s| {
            s.trim_start().starts_with("[Unreleased]") || s.trim_start().starts_with("Unreleased")
        });

        if !is_unreleased {
            version_count += 1;
        }

        if version_count >= max_versions {
            // Find the next ## after this one to split there
            let after = &content[header_start + 3..];
            if let Some(next) = after.find("\n## ") {
                let split_at = header_start + 3 + next + 1; // +1 to include the \n
                return (&content[..split_at], &content[split_at..]);
            }
            break;
        }

        search_start = abs_pos + 4; // skip past "\n## "
    }

    (content, "")
}

/// Returns today's date as YYYY-MM-DD using std only.
fn today_iso() -> String {
    // Use UNIX_EPOCH + SystemTime to get days, then Hinnant civil_from_days
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let days = (secs / 86400) as i64;

    // Hinnant's civil_from_days (epoch = 1970-01-01 = day 0)
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Debug, Clone, Copy)]
struct ChangelogSection {
    body_start: usize,
    body_end: usize,
}

fn is_unreleased_header(line: &str) -> bool {
    matches!(line.trim(), "## [Unreleased]" | "## Unreleased")
}

fn is_forbidden_unreleased_target_header(line: &str) -> bool {
    matches!(
        line.trim(),
        "## [Unreleased]" | "## Unreleased" | "## [HEAD]" | "## HEAD"
    )
}

fn find_unreleased_section(contents: &str) -> miette::Result<Option<ChangelogSection>> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in contents.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        lines.push((start, offset, line));
    }

    let mut unreleased_index = None;
    for (index, (_, _, line)) in lines.iter().enumerate() {
        if is_unreleased_header(line) && unreleased_index.replace(index).is_some() {
            return Err(miette::miette!(
                "CHANGELOG.md contains multiple Unreleased sections; cannot update safely."
            ));
        }
    }

    let Some(index) = unreleased_index else {
        return Ok(None);
    };

    let body_start = lines[index].1;
    let mut body_end = contents.len();
    for (start, _, line) in lines.iter().skip(index + 1) {
        if line.trim().starts_with("## ") {
            body_end = *start;
            break;
        }
    }

    Ok(Some(ChangelogSection {
        body_start,
        body_end,
    }))
}

fn normalize_generated_changelog_body(generated: &str) -> String {
    generated
        .lines()
        .filter(|line| !is_forbidden_unreleased_target_header(line))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn new_unreleased_changelog(generated: &str) -> String {
    let body = normalize_generated_changelog_body(generated);
    if body.is_empty() {
        "# Changelog\n\n## [Unreleased]\n".to_string()
    } else {
        format!("# Changelog\n\n## [Unreleased]\n\n{body}\n")
    }
}

fn replace_unreleased_section(existing: &str, generated: &str) -> miette::Result<String> {
    let Some(section) = find_unreleased_section(existing)? else {
        return Err(miette::miette!(
            "CHANGELOG.md exists but does not contain ## [Unreleased] or ## Unreleased; add an Unreleased section before running `communique generate HEAD --changelog`."
        ));
    };

    let body = normalize_generated_changelog_body(generated);
    let mut updated = existing[..section.body_start].to_string();
    if !updated.ends_with('\n') {
        updated.push('\n');
    }

    if !body.is_empty() {
        updated.push('\n');
        updated.push_str(&body);
        updated.push('\n');
    }

    let tail = existing[section.body_end..].trim_start_matches(['\r', '\n']);
    if !tail.is_empty() {
        updated.push('\n');
        updated.push_str(tail);
    }

    Ok(format!("{}\n", updated.trim_end()))
}

fn read_unreleased_section(repo_root: &Path) -> miette::Result<Option<String>> {
    let path = repo_root.join("CHANGELOG.md");
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(crate::error::Error::Io(err).into()),
    };

    let Some(section) = find_unreleased_section(&contents)? else {
        return Ok(None);
    };

    Ok(Some(
        contents[section.body_start..section.body_end]
            .trim()
            .to_string(),
    ))
}

async fn update_changelog(
    ctx: &Context,
    parsed: &ParsedOutput,
    dry_run: bool,
    job: &Arc<ProgressJob>,
) -> miette::Result<()> {
    job.prop("message", "Updating CHANGELOG.md...");

    let changelog_path = ctx.repo_root.join("CHANGELOG.md");
    if ctx.is_unreleased_head() {
        debug_assert_eq!(ctx.tag, "HEAD");
        let updated = match std::fs::read_to_string(&changelog_path) {
            Ok(existing) => replace_unreleased_section(&existing, &parsed.changelog)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                new_unreleased_changelog(&parsed.changelog)
            }
            Err(err) => return Err(crate::error::Error::Io(err).into()),
        };

        if !dry_run {
            xx::file::write(&changelog_path, updated)?;
            info!("wrote {}", changelog_path.display());
        }

        return Ok(());
    }

    let existing = xx::file::read_to_string(&changelog_path)
        .unwrap_or_else(|_| "# Changelog\n\n## [Unreleased]\n".to_string());

    let (head, tail) = split_changelog(&existing, 3);

    let date = today_iso();
    let release_url = format!(
        "https://github.com/{}/releases/tag/{}",
        ctx.owner_repo, ctx.tag
    );

    let system = "\
You are a precise CHANGELOG.md editor. Given the top portion of an existing \
CHANGELOG.md and a new version entry, produce the updated content.

Rules:
- Match the formatting conventions of the existing file (header style, spacing, link patterns)
- Insert the new version section after any [Unreleased] section, before existing version entries
- If an entry for this exact version already exists, replace it with the new content
- Use the date and release URL provided to format the version header
- If the file uses linked headers like ## [X.Y.Z](url) - date, follow that pattern
- If the file uses plain headers like ## X.Y.Z, follow that pattern
- Preserve the [Unreleased] section header (keep it even if empty)
- Preserve all other existing entries exactly as-is
- Output ONLY the raw markdown content — no code fences, no explanations";

    let user_msg = format!(
        "Version: {tag}\nDate: {date}\nRelease URL: {release_url}\n\n\
         New changelog entry:\n{changelog}\n\n\
         Current CHANGELOG.md (top portion):\n{head}",
        tag = ctx.tag,
        changelog = parsed.changelog,
        head = head,
    );

    let mut conv = ctx.client.new_conversation(&user_msg);
    let response = ctx.client.send_turn(system, &mut conv, &[]).await?;

    let updated_head = response.text.unwrap_or_default();

    info!(
        "changelog update tokens: {} input + {} output",
        response.usage.input_tokens, response.usage.output_tokens
    );

    if !dry_run {
        let full = if tail.is_empty() {
            updated_head
        } else {
            format!("{updated_head}{tail}")
        };
        let content = format!("{}\n", full.trim_end());
        xx::file::write(&changelog_path, content)?;
        info!("wrote {}", changelog_path.display());
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
