use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum Error {
    #[error("git error: {0}")]
    Git(String),

    #[error("GitHub API error: {0}")]
    GitHub(String),

    #[error("Anthropic API error: {0}")]
    Anthropic(String),

    #[error("tool error: {0}")]
    Tool(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
