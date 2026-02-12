use clap::{Parser, Subcommand};

use crate::usage;

#[derive(Parser, Debug)]
#[command(
    name = "communique",
    version,
    about = "Editorialized release notes powered by AI"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate release notes for a git tag
    Generate {
        /// Git tag to generate release notes for
        tag: String,

        /// Previous tag (auto-detected if omitted)
        prev_tag: Option<String>,

        /// Push editorialized notes to the GitHub release
        #[arg(long)]
        github_release: bool,

        /// Output concise changelog entry instead of detailed notes
        #[arg(long)]
        concise: bool,

        /// GitHub repo in owner/repo format (auto-detected from git remote)
        #[arg(long)]
        repo: Option<String>,

        /// Anthropic model to use
        #[arg(long)]
        model: Option<String>,

        /// Max response tokens
        #[arg(long)]
        max_tokens: Option<u32>,
    },

    /// Generate a communique.toml config file in the repo root
    Init {
        /// Overwrite existing config file
        #[arg(long)]
        force: bool,
    },

    Usage(usage::Usage),
}
