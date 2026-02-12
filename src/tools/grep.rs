use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::error::{Error, Result};
use crate::llm::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "grep".into(),
        description: "Search file contents using ripgrep (rg). Returns matching lines with file paths and line numbers.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "glob": {
                    "type": "string",
                    "description": "Optional file glob to restrict search (e.g. '*.rs')"
                }
            },
            "required": ["pattern"]
        }),
    }
}

pub fn execute(repo_root: &Path, input: &serde_json::Value) -> Result<String> {
    let pattern = input["pattern"]
        .as_str()
        .ok_or_else(|| Error::Tool("grep: missing 'pattern' parameter".into()))?;

    let mut cmd = Command::new("rg");
    cmd.args([
        "--no-config",
        "--line-number",
        "--no-heading",
        "--max-count",
        "50",
    ])
    .arg(pattern)
    .current_dir(repo_root);

    if let Some(glob) = input.get("glob").and_then(|v| v.as_str()) {
        cmd.args(["--glob", glob]);
    }

    let output = cmd
        .output()
        .map_err(|e| Error::Tool(format!("grep: {e}")))?;

    // Exit code 2+ is an actual error; rg returns 1 for no matches
    if output.status.code().is_some_and(|c| c >= 2) {
        return Err(Error::Tool(format!(
            "grep: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let result = String::from_utf8_lossy(&output.stdout);
    if result.is_empty() {
        Ok("No matches found.".into())
    } else {
        Ok(result.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TempRepo;
    use serde_json::json;

    #[test]
    fn test_grep_basic() {
        let repo = TempRepo::new();
        repo.write_file("hello.txt", "hello world\nfoo bar");
        repo.commit("init");

        let result = execute(repo.path(), &json!({"pattern": "hello"})).unwrap();
        assert!(
            result.contains("hello world"),
            "expected 'hello world' in: {result:?}"
        );
    }

    #[test]
    fn test_grep_no_matches() {
        let repo = TempRepo::new();
        repo.write_file("hello.txt", "hello world");
        repo.commit("init");

        let result = execute(repo.path(), &json!({"pattern": "zzzzz"})).unwrap();
        assert_eq!(result, "No matches found.");
    }

    #[test]
    fn test_grep_with_glob() {
        let repo = TempRepo::new();
        repo.write_file("a.rs", "fn main() {}");
        repo.write_file("b.txt", "fn main() {}");
        repo.commit("init");

        let result = execute(repo.path(), &json!({"pattern": "fn main", "glob": "*.rs"})).unwrap();
        assert!(result.contains("a.rs"), "expected 'a.rs' in: {result:?}");
        assert!(
            !result.contains("b.txt"),
            "unexpected 'b.txt' in: {result:?}"
        );
    }

    #[test]
    fn test_grep_missing_pattern() {
        let repo = TempRepo::new();
        let err = execute(repo.path(), &json!({})).unwrap_err();
        assert!(err.to_string().contains("missing 'pattern'"));
    }
}
