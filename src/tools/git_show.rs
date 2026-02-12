use std::path::Path;

use serde_json::json;
use xx::process;

use crate::error::{Error, Result};
use crate::llm::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "git_show".into(),
        description: "Show full details of a commit (message, author, diff).".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "ref": {
                    "type": "string",
                    "description": "Commit SHA, tag, branch, or other git ref"
                }
            },
            "required": ["ref"]
        }),
    }
}

pub fn execute(repo_root: &Path, input: &serde_json::Value) -> Result<String> {
    let git_ref = input["ref"]
        .as_str()
        .ok_or_else(|| Error::Tool("git_show: missing 'ref' parameter".into()))?;

    let output = process::cmd("git", ["show", "--stat", "--patch", git_ref])
        .cwd(repo_root)
        .read()?;

    // Truncate very large output to avoid blowing up context
    if output.len() > 50_000 {
        Ok(format!(
            "{}...\n\n[output truncated at 50KB]",
            &output[..50_000]
        ))
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
    fn test_git_show() {
        let repo = TempRepo::new();
        repo.write_file("hello.txt", "hello world");
        repo.commit("Add hello file");

        let result = execute(repo.path(), &json!({"ref": "HEAD"})).unwrap();
        assert!(result.contains("Add hello file"));
        assert!(result.contains("hello.txt"));
    }

    #[test]
    fn test_git_show_missing_ref() {
        let repo = TempRepo::new();
        repo.write_file("hello.txt", "hello");
        repo.commit("init");

        let err = execute(repo.path(), &json!({"ref": "nonexistent_ref_abc123"})).unwrap_err();
        assert!(err.to_string().contains("nonexistent_ref_abc123"));
    }
}
