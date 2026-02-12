#![allow(unused_assignments)]

use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum Error {
    #[error("git error: {0}")]
    Git(String),

    #[error("GitHub API error: {0}")]
    GitHub(String),

    #[error("LLM API error: {0}")]
    Llm(String),

    #[error("tool error: {0}")]
    Tool(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("{message}")]
    #[diagnostic()]
    Toml {
        message: String,
        #[source_code]
        src: miette::NamedSource<String>,
        #[label("{message}")]
        span: miette::SourceSpan,
    },

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Xx(#[from] xx::XXError),
}

pub type Result<T> = std::result::Result<T, Error>;
