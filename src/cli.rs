use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "communique", about = "Editorialized release notes powered by Claude")]
pub struct Cli {
    /// Git tag to generate release notes for
    pub tag: String,

    /// Previous tag (auto-detected if omitted)
    pub prev_tag: Option<String>,

    /// Push editorialized notes to the GitHub release
    #[arg(long)]
    pub github_release: bool,

    /// Output concise changelog entry instead of detailed notes
    #[arg(long)]
    pub concise: bool,

    /// GitHub repo in owner/repo format (auto-detected from git remote)
    #[arg(long)]
    pub repo: Option<String>,

    /// Anthropic model to use
    #[arg(long, default_value = "claude-opus-4-6")]
    pub model: String,

    /// Max response tokens
    #[arg(long, default_value_t = 4096)]
    pub max_tokens: u32,
}
