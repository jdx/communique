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
    pub emoji: Option<bool>,
    pub verify_links: Option<bool>,
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
#emoji = true
#verify_links = true
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
