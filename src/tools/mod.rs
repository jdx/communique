pub mod get_pr;
pub mod get_pr_diff;
pub mod grep;
pub mod list_files;
pub mod read_file;
pub mod submit_release_notes;

use std::path::Path;

use crate::error::Result;
use crate::github::GitHubClient;
use crate::llm::ToolDefinition;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_all_definitions_without_github() {
        let defs = all_definitions(false);
        assert_eq!(defs.len(), 4);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"list_files"));
        assert!(names.contains(&"grep"));
        assert!(names.contains(&"submit_release_notes"));
    }

    #[test]
    fn test_all_definitions_with_github() {
        let defs = all_definitions(true);
        assert_eq!(defs.len(), 6);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"get_pr"));
        assert!(names.contains(&"get_pr_diff"));
    }

    #[tokio::test]
    async fn test_dispatch_unknown_tool() {
        let tmp = std::env::temp_dir();
        let err = dispatch("nonexistent_tool", &json!({}), &tmp, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown tool"));
    }

    #[tokio::test]
    async fn test_dispatch_get_pr_without_github() {
        let tmp = std::env::temp_dir();
        let err = dispatch("get_pr", &json!({"number": 1}), &tmp, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("GITHUB_TOKEN"));
    }
}
