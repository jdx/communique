use std::path::PathBuf;

use usage_rs::{Args, Cli, Subcommands};

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
    /// Generate a self-contained shell completion script
    #[usage(effect = "read")]
    Completion {
        /// Shell: bash, zsh, fish, or powershell
        #[usage(arg)]
        shell: String,
    },
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
#[usage(completion = true)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn typed_commands_bind_their_fields() {
        let argv: &[&OsStr] = &[
            OsStr::new("--verbose"),
            OsStr::new("generate"),
            OsStr::new("v1.2.3"),
            OsStr::new("v1.2.2"),
            OsStr::new("--github-release"),
            OsStr::new("--max-tokens"),
            OsStr::new("2048"),
            OsStr::new("--provider"),
            OsStr::new("openai"),
        ];
        let cli = Cli::parse_from(argv).expect("generate should parse");
        assert!(cli.verbose);
        check_command(cli.command, "generate");

        for (argv, expected) in [
            (&[OsStr::new("init"), OsStr::new("--force")][..], "init"),
            (&[OsStr::new("sponsors")][..], "sponsors"),
            (&[OsStr::new("usage")][..], "usage"),
            (
                &[OsStr::new("completion"), OsStr::new("bash")][..],
                "completion",
            ),
        ] {
            let cli = Cli::parse_from(argv).expect("command should parse");
            check_command(cli.command, expected);
        }
    }

    /// A second command line merges into a parsed value rather than replacing it,
    /// which is what keeps globals set on the first line in force.
    #[test]
    fn a_later_command_line_merges_into_the_parsed_value() {
        let mut cli = Cli::parse_from(&[OsStr::new("--verbose"), OsStr::new("sponsors")])
            .expect("sponsors should parse");

        cli.try_update_from(&[
            OsStr::new("generate"),
            OsStr::new("v1.2.3"),
            OsStr::new("v1.2.2"),
            OsStr::new("--github-release"),
            OsStr::new("--max-tokens"),
            OsStr::new("2048"),
            OsStr::new("--provider"),
            OsStr::new("openai"),
        ])
        .expect("the second command line should merge");

        assert!(cli.verbose, "the standing global survives the merge");
        check_command(cli.command, "generate");
    }

    fn check_command(command: Command, expected: &str) {
        match command {
            Command::Generate(generate) => {
                assert_eq!(expected, "generate");
                assert_eq!(generate.tag, "v1.2.3");
                assert_eq!(generate.prev_tag.as_deref(), Some("v1.2.2"));
                assert!(generate.github_release);
                assert_eq!(generate.max_tokens, Some(2048));
                assert_eq!(generate.provider, Some(Provider::OpenAI));
            }
            Command::Init(init) => {
                assert_eq!(expected, "init");
                assert!(init.force);
            }
            Command::Sponsors => assert_eq!(expected, "sponsors"),
            Command::Usage(_) => assert_eq!(expected, "usage"),
            Command::Completion { shell } => {
                assert_eq!(expected, "completion");
                assert_eq!(shell, "bash");
            }
        }
    }
}
