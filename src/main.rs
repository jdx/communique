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
    let cli = parse_args();

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

    // A root with no subcommand prints help and stops, which is what clap did for a CLI
    // whose `command` was not optional.
    let Some(command) = cli.command else {
        print!(
            "{}",
            usage_argv::help::render(Cli::spec(), Cli::command(), false)
                .expect("the root is this CLI's own")
        );
        return Ok(());
    };

    let result = match command {
        Command::Usage(usage) => usage.run(),
        // Destructured rather than ignored: `Sponsors` has no fields, and a `_` leaves the
        // variant's payload unread, which is a `dead_code` warning and an error in CI. A
        // command that takes nothing still needs a struct here — clap allowed a bare variant.
        Command::Sponsors(args) => {
            let cli::Sponsors {} = *args;
            sponsors()
        }
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

/// The command line, or a clap-shaped message and a non-zero exit.
///
/// `Cli::parse` renders a failure with `{:?}`, which is the error's *shape* rather than
/// something to read. usage-argv has the rendering — `diagnostic::render` is held to clap's
/// wording on purpose — so this reaches for it directly. Worth pushing back into the derive.
fn parse_args() -> Cli {
    let raw: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let argv: Vec<&std::ffi::OsStr> = raw.iter().map(|a| a.as_os_str()).collect();
    match Cli::parse_from(&argv) {
        Ok(cli) => cli,
        // Not a failure: someone asked a question, and the answer goes to stdout.
        Err(usage_argv::Error::Help { cmd, long }) => {
            match usage_argv::help::render(Cli::spec(), cmd, long) {
                Some(page) => print!("{page}"),
                None => unreachable!("help was asked for a command this program does not have"),
            }
            std::process::exit(0);
        }
        Err(e) => {
            eprint!(
                "{}",
                usage_argv::diagnostic::render(
                    Cli::spec(),
                    &argv,
                    &e,
                    usage_argv::diagnostic::Style::auto(),
                )
            );
            std::process::exit(2);
        }
    }
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
        "communique and the jdx.dev open source tools are sponsored by:\n\n  entire.io - https://entire.io\n  37signals - https://37signals.com\n\nView all sponsors: https://jdx.dev/sponsors.html"
    );
    Ok(())
}
