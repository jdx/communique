use crate::cli::Cli;
use clap::CommandFactory;

/// Generates a usage spec for the CLI
///
/// https://usage.jdx.dev
#[derive(Debug, clap::Args)]
#[clap(hide = true, verbatim_doc_comment)]
pub struct Usage {}

impl Usage {
    pub fn run(&self) -> miette::Result<()> {
        let mut cmd = Cli::command();
        clap_usage::generate(&mut cmd, "communique", &mut std::io::stdout());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_to_string() -> String {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        clap_usage::generate(&mut cmd, "communique", &mut buf);
        String::from_utf8(buf).expect("usage output is valid UTF-8")
    }

    #[test]
    fn test_usage_kdl_in_sync() {
        let generated = generate_to_string();
        let on_disk = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("communique.usage.kdl"),
        )
        .expect("communique.usage.kdl should exist");
        assert_eq!(
            generated.trim(),
            on_disk.trim(),
            "communique.usage.kdl is out of sync — run `cargo run -- usage > communique.usage.kdl` to update"
        );
    }
}
