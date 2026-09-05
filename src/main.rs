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
mod retry;
mod tools;
mod usage;

#[cfg(test)]
mod test_helpers;

use std::time::Duration;

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
        Command::Completion { shell } => {
            let shell = usage_rs::complete::Shell::from_name(&shell)
                .ok_or_else(|| miette::miette!("unsupported shell: {shell}"))?;
            print!("{}", Cli::completion_script(shell));
            Ok(())
        }
        Command::Usage(usage) => usage.run(),
        Command::Sponsors => sponsors(),
        Command::Init(init_args) => init(init_args.force),
        Command::Generate(g) => {
            generate::run(generate::GenerateOptions {
                tag: g.tag,
                prev_tag: g.prev_tag,
                github_release: g.github_release,
                changelog: g.changelog,
                concise: g.concise,
                dry_run: g.dry_run,
                repo: g.repo,
                model: g.model,
                max_tokens: g.max_tokens,
                provider: g.provider,
                base_url: g.base_url,
                output: g.output,
                config: cli.config,
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

fn sponsors() -> miette::Result<()> {
    println!(
        "communique and the jdx.dev open source tools are sponsored by:\n\n  entire.io - https://entire.io\n  Omacom Foundation - https://omarchy.org/patrons/\n\nView all sponsors: https://jdx.dev/sponsors.html"
    );
    Ok(())
}
