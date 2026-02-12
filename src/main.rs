mod agent;
mod cli;
mod config;
mod error;
mod generate;
mod git;
mod github;
mod links;
mod llm;
mod output;
mod prompt;
mod providers;
mod tools;
mod usage;

#[cfg(test)]
mod test_helpers;

use std::time::Duration;

use clap::Parser;
use log::LevelFilter;
use miette::IntoDiagnostic;

use cli::{Cli, Command};
use config::Config;

#[tokio::main]
async fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    if cli.quiet {
        // SAFETY: called before spawning any threads (pre-tokio runtime work)
        unsafe { std::env::set_var("CLX_NO_PROGRESS", "1") };
    }

    let level = if let Ok(rust_log) = std::env::var("RUST_LOG") {
        rust_log.parse().unwrap_or(LevelFilter::Info)
    } else if cli.verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Warn
    };
    let _ = clx::progress::ProgressLogger::new(level).init();

    clx::progress::set_interval(Duration::from_millis(100));
    if !console::user_attended_stderr() {
        clx::progress::set_output(clx::progress::ProgressOutput::Text);
    }

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
            provider,
            base_url,
            output,
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
                provider,
                base_url,
                output,
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
