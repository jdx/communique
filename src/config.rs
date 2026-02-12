use std::path::Path;

use serde::Deserialize;

use crate::error::Result;

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub system_extra: Option<String>,
    pub context: Option<String>,
    pub defaults: Option<Defaults>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Defaults {
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub repo: Option<String>,
    pub provider: Option<String>,
    pub base_url: Option<String>,
    pub emoji: Option<bool>,
    pub verify_links: Option<bool>,
    pub match_style: Option<bool>,
}

const TEMPLATE: &str = r#"# Extra instructions appended to the system prompt.
# Use this to customize tone, style, or project-specific conventions.
#system_extra = ""

# Extra context included in every user prompt.
# Useful for project descriptions or recurring context.
#context = ""

[defaults]
#model = "claude-opus-4-6"
#max_tokens = 4096
#repo = "owner/repo"
#provider = "anthropic"
#base_url = ""
#emoji = true
#verify_links = true
#match_style = true
"#;

impl Config {
    pub fn load(repo_root: &Path) -> Result<Option<Config>> {
        let path = repo_root.join("communique.toml");
        if !path.exists() {
            return Ok(None);
        }
        let contents = xx::file::read_to_string(&path)?;
        let config: Config = toml::from_str(&contents).map_err(|e| {
            let span = e.span().map(|s| s.into()).unwrap_or((0, 0).into());
            crate::error::Error::Toml {
                message: e.message().to_string(),
                src: miette::NamedSource::new(path.display().to_string(), contents.clone()),
                span,
            }
        })?;
        Ok(Some(config))
    }

    pub fn template() -> &'static str {
        TEMPLATE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = Config::load(dir.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_valid_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("communique.toml"),
            r#"
system_extra = "Be concise"
context = "A CLI tool"

[defaults]
model = "gpt-4"
emoji = false
"#,
        )
        .unwrap();
        let config = Config::load(dir.path()).unwrap().unwrap();
        assert_eq!(config.system_extra.as_deref(), Some("Be concise"));
        assert_eq!(config.context.as_deref(), Some("A CLI tool"));
        let defaults = config.defaults.unwrap();
        assert_eq!(defaults.model.as_deref(), Some("gpt-4"));
        assert_eq!(defaults.emoji, Some(false));
    }

    #[test]
    fn test_load_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("communique.toml"), "invalid {{{{").unwrap();
        let err = Config::load(dir.path());
        assert!(err.is_err());
    }

    #[test]
    fn test_template_is_valid_toml() {
        let config: Config = toml::from_str(Config::template()).unwrap();
        assert!(config.system_extra.is_none());
    }
}
