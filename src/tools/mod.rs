pub mod get_pr;
pub mod get_pr_diff;
pub mod grep;
pub mod list_files;
pub mod read_file;
pub mod submit_release_notes;

use std::path::Path;

use crate::anthropic::ToolDefinition;
use crate::error::Result;
use crate::github::GitHubClient;

pub fn all_definitions(has_github: bool) -> Vec<ToolDefinition> {
    let mut defs = vec![
        read_file::definition(),
        list_files::definition(),
        grep::definition(),
        submit_release_notes::definition(),
    ];
    if has_github {
        defs.push(get_pr::definition());
        defs.push(get_pr_diff::definition());
    }
    defs
}

pub async fn dispatch(
    name: &str,
    input: &serde_json::Value,
    repo_root: &Path,
    github: Option<&GitHubClient>,
) -> Result<String> {
    match name {
        "read_file" => read_file::execute(repo_root, input),
        "list_files" => list_files::execute(repo_root, input),
        "grep" => grep::execute(repo_root, input),
        "get_pr" => {
            let gh = github.ok_or_else(|| {
                crate::error::Error::Tool("get_pr requires GITHUB_TOKEN to be set".into())
            })?;
            get_pr::execute(gh, input).await
        }
        "get_pr_diff" => {
            let gh = github.ok_or_else(|| {
                crate::error::Error::Tool("get_pr_diff requires GITHUB_TOKEN to be set".into())
            })?;
            get_pr_diff::execute(gh, input).await
        }
        _ => Err(crate::error::Error::Tool(format!("unknown tool: {name}"))),
    }
}
