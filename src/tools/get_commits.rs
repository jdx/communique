use std::path::Path;

use serde_json::json;
use xx::process;

use crate::error::Result;
use crate::llm::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_commits".into(),
        description: "List commits between refs or for a specific file path.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "from": {
                    "type": "string",
                    "description": "Start ref (exclusive). If omitted, shows recent commits."
                },
                "to": {
                    "type": "string",
                    "description": "End ref (inclusive). Defaults to HEAD."
                },
                "path": {
                    "type": "string",
                    "description": "Filter to commits touching this path"
                }
            }
        }),
    }
}

pub fn execute(repo_root: &Path, input: &serde_json::Value) -> Result<String> {
    let from = input.get("from").and_then(|v| v.as_str());
    let to = input.get("to").and_then(|v| v.as_str()).unwrap_or("HEAD");
    let path = input.get("path").and_then(|v| v.as_str());

    let mut cmd = process::cmd(
        "git",
        [
            "log",
            "--pretty=format:%h %an %ad %s",
            "--date=short",
            "-n",
            "200",
        ],
    )
    .cwd(repo_root);

    if let Some(from) = from {
        cmd = cmd.arg(format!("{from}..{to}"));
    } else {
        cmd = cmd.arg(to);
    }

    if let Some(path) = path {
        cmd = cmd.args(["--", path]);
    }

    let output = cmd.read()?;
    if output.is_empty() {
        Ok("No commits found.".into())
    } else {
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TempRepo;
    use serde_json::json;

    #[test]
    fn test_get_commits_range() {
        let repo = TempRepo::new();
        repo.write_file("a.txt", "a");
        repo.commit("first commit");
        repo.tag("v1");

        repo.write_file("b.txt", "b");
        repo.commit("second commit");

        repo.write_file("c.txt", "c");
        repo.commit("third commit");
        repo.tag("v2");

        let result = execute(repo.path(), &json!({"from": "v1", "to": "v2"})).unwrap();
        assert!(result.contains("second commit"));
        assert!(result.contains("third commit"));
        assert!(!result.contains("first commit"));
    }

    #[test]
    fn test_get_commits_with_path() {
        let repo = TempRepo::new();
        repo.write_file("a.txt", "a");
        repo.commit("change a");

        repo.write_file("b.txt", "b");
        repo.commit("change b");

        let result = execute(repo.path(), &json!({"path": "a.txt"})).unwrap();
        assert!(result.contains("change a"));
        assert!(!result.contains("change b"));
    }

    #[test]
    fn test_get_commits_defaults() {
        let repo = TempRepo::new();
        repo.write_file("a.txt", "a");
        repo.commit("initial commit");

        let result = execute(repo.path(), &json!({})).unwrap();
        assert!(result.contains("initial commit"));
    }
}
