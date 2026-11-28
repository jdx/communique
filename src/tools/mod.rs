pub mod get_commits;
pub mod get_issue;
pub mod get_pr;
pub mod get_pr_diff;
pub mod git_show;
pub mod grep;
pub mod list_files;
pub mod read_file;
pub mod submit_release_notes;

use std::collections::HashMap;
use std::path::Path;

use crate::error::Result;
use crate::forge::{ForgeClient, ForgeKind};
use crate::llm::ToolDefinition;

/// In-memory cache for tool call results, keyed by (tool_name, input_json).
/// Only successful results are cached. Avoids redundant file reads, git
/// operations, and GitHub/GitLab API calls when the LLM calls the same tool with
/// identical arguments across iterations.
#[derive(Default)]
pub struct ToolCache {
    entries: HashMap<String, String>,
}

impl ToolCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str, input: &serde_json::Value) -> Option<&str> {
        self.entries
            .get(&Self::key(name, input))
            .map(|s| s.as_str())
    }

    pub fn insert(&mut self, name: &str, input: &serde_json::Value, result: String) {
        self.entries.insert(Self::key(name, input), result);
    }

    fn key(name: &str, input: &serde_json::Value) -> String {
        format!("{name}\0{input}")
    }
}

/// `forge` is the forge whose API is available; its pull/merge request and
/// issue tools are offered only when a token for it is set.
pub fn all_definitions(
    forge: Option<ForgeKind>,
    include_release_notes: bool,
    include_changelog: bool,
) -> Vec<ToolDefinition> {
    let mut defs = vec![
        read_file::definition(),
        list_files::definition(),
        grep::definition(),
        git_show::definition(),
        get_commits::definition(),
        submit_release_notes::definition(include_release_notes, include_changelog),
    ];
    if let Some(forge) = forge {
        defs.push(get_pr::definition(forge));
        defs.push(get_pr_diff::definition(forge));
        defs.push(get_issue::definition(forge));
    }
    defs
}

pub async fn dispatch(
    name: &str,
    input: &serde_json::Value,
    repo_root: &Path,
    forge: Option<&ForgeClient>,
) -> Result<String> {
    match name {
        "read_file" => read_file::execute(repo_root, input),
        "list_files" => list_files::execute(repo_root, input),
        "grep" => grep::execute(repo_root, input),
        "git_show" => git_show::execute(repo_root, input),
        "get_commits" => get_commits::execute(repo_root, input),
        "get_pr" => {
            let forge = forge.ok_or_else(|| {
                crate::error::Error::Tool(
                    "get_pr requires GITHUB_TOKEN (or GITLAB_TOKEN on GitLab) to be set".into(),
                )
            })?;
            get_pr::execute(forge, input).await
        }
        "get_pr_diff" => {
            let forge = forge.ok_or_else(|| {
                crate::error::Error::Tool(
                    "get_pr_diff requires GITHUB_TOKEN (or GITLAB_TOKEN on GitLab) to be set"
                        .into(),
                )
            })?;
            get_pr_diff::execute(forge, input).await
        }
        "get_issue" => {
            let forge = forge.ok_or_else(|| {
                crate::error::Error::Tool(
                    "get_issue requires GITHUB_TOKEN (or GITLAB_TOKEN on GitLab) to be set".into(),
                )
            })?;
            get_issue::execute(forge, input).await
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
        let defs = all_definitions(None, true, false);
        assert_eq!(defs.len(), 6);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"list_files"));
        assert!(names.contains(&"grep"));
        assert!(names.contains(&"git_show"));
        assert!(names.contains(&"get_commits"));
        assert!(names.contains(&"submit_release_notes"));
    }

    #[test]
    fn test_all_definitions_with_github() {
        let defs = all_definitions(Some(ForgeKind::GitHub), true, true);
        assert_eq!(defs.len(), 9);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"get_pr"));
        assert!(names.contains(&"get_pr_diff"));
        assert!(names.contains(&"get_issue"));
    }

    #[test]
    fn test_all_definitions_with_gitlab_describe_merge_requests() {
        let defs = all_definitions(Some(ForgeKind::GitLab), true, true);
        assert_eq!(defs.len(), 9);
        for def in defs.iter().filter(|d| d.name.starts_with("get_")) {
            assert!(!def.description.contains("GitHub"), "{}", def.description);
        }
        let get_pr = defs.iter().find(|d| d.name == "get_pr").unwrap();
        assert!(get_pr.description.contains("GitLab merge request"));
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

    #[tokio::test]
    async fn test_dispatch_get_issue_without_github() {
        let tmp = std::env::temp_dir();
        let err = dispatch("get_issue", &json!({"number": 1}), &tmp, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("GITHUB_TOKEN"));
    }

    #[test]
    fn test_tool_cache_miss_and_hit() {
        let mut cache = ToolCache::new();
        let input = json!({"path": "README.md"});
        assert!(cache.get("read_file", &input).is_none());

        cache.insert("read_file", &input, "file contents".into());
        assert_eq!(cache.get("read_file", &input), Some("file contents"));
    }

    #[test]
    fn test_tool_cache_different_args() {
        let mut cache = ToolCache::new();
        let input_a = json!({"path": "a.txt"});
        let input_b = json!({"path": "b.txt"});

        cache.insert("read_file", &input_a, "aaa".into());
        assert_eq!(cache.get("read_file", &input_a), Some("aaa"));
        assert!(cache.get("read_file", &input_b).is_none());
    }

    #[test]
    fn test_tool_cache_different_tools_same_args() {
        let mut cache = ToolCache::new();
        let input = json!({"pattern": "foo"});

        cache.insert("grep", &input, "grep result".into());
        assert_eq!(cache.get("grep", &input), Some("grep result"));
        assert!(cache.get("list_files", &input).is_none());
    }
}
