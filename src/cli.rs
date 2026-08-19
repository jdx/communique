use std::path::PathBuf;

use usage_derive::{Args, Cli, Subcommands};

use crate::providers::Provider;
use crate::usage;

/// Generate release notes for a git tag
#[derive(Debug, Args)]
#[usage(effect = "read")]
pub struct Generate {
    /// Git tag to generate release notes for
    #[usage(arg)]
    pub tag: String,

    /// Previous tag (auto-detected if omitted)
    #[usage(arg)]
    pub prev_tag: Option<String>,

    /// Push editorialized notes to the GitHub release
    #[usage(long, effect = "write")]
    pub github_release: bool,

    /// Update CHANGELOG.md with the generated changelog entry
    #[usage(long, effect = "write")]
    pub changelog: bool,

    /// Output concise changelog entry instead of detailed notes
    #[usage(long)]
    pub concise: bool,

    /// Generate notes without updating GitHub or verifying links
    #[usage(long, short = 'n')]
    pub dry_run: bool,

    /// GitHub repo in owner/repo format (auto-detected from git remote)
    #[usage(long)]
    pub repo: Option<String>,

    /// LLM model to use
    #[usage(long)]
    pub model: Option<String>,

    /// Max response tokens
    #[usage(long)]
    pub max_tokens: Option<u32>,

    /// LLM provider (anthropic or openai, auto-detected from model if omitted)
    #[usage(long)]
    pub provider: Option<Provider>,

    /// Base URL for the LLM API
    #[usage(long)]
    pub base_url: Option<String>,

    /// Write output to a file instead of stdout
    #[usage(long, short, effect = "write")]
    pub output: Option<PathBuf>,
}

/// Generate a communique.toml config file in the repo root
#[derive(Debug, Args)]
#[usage(effect = "write")]
pub struct Init {
    /// Overwrite existing config file
    #[usage(long, effect = "destructive")]
    pub force: bool,
}

#[derive(Subcommands)]
pub enum Command {
    /// Generate release notes for a git tag
    Generate(Box<Generate>),
    /// Generate a communique.toml config file in the repo root
    Init(Box<Init>),
    /// Show the companies sponsoring communique and the jdx.dev open source tools
    #[usage(effect = "read")]
    Sponsors,
    #[usage(hide)]
    Usage(Box<usage::Usage>),
}

/// Editorialized release notes powered by AI
#[derive(Cli)]
#[usage(
    name = "communique",
    bin = "communique",
    version,
    usage = "Usage: communique [OPTIONS] <COMMAND>",
    min_usage_version = "4.0",
    unknown_flags = "error"
)]
pub struct Cli {
    #[usage(subcommand)]
    pub command: Command,

    /// Enable verbose logging output
    #[usage(long, short, global)]
    pub verbose: bool,

    /// Suppress progress output
    #[usage(long, short, global)]
    pub quiet: bool,

    /// Path to config file (default: communique.toml in repo root)
    #[usage(long, short, global)]
    pub config: Option<PathBuf>,
}
