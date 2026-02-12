mod agent;
mod anthropic;
mod cli;
mod config;
mod error;
mod generate;
mod git;
mod github;
mod links;
mod output;
mod prompt;
mod tools;
mod usage;

use std::time::Duration;

use clap::Parser;
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
        Command::Init { force } => init(force),
        Command::Generate {
            tag,
            prev_tag,
            github_release,
            concise,
            dry_run,
            repo,
            model,
            max_tokens,
        } => {
            generate::run(generate::GenerateOptions {
                tag,
                prev_tag,
                github_release,
                concise,
                dry_run,
                repo,
                model,
                max_tokens,
            })
            .await
        }
    };

    clx::progress::flush();
    result
}

fn init(force: bool) -> miette::Result<()> {
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
