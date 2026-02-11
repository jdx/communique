use std::path::Path;

use serde_json::json;
use xx::process;

use crate::anthropic::ToolDefinition;
use crate::error::{Error, Result};

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

    let mut cmd = process::cmd("rg", ["--line-number", "--no-heading", "--max-count", "50"])
        .arg(pattern)
        .cwd(repo_root)
        .unchecked(); // rg returns exit code 1 for no matches

    if let Some(glob) = input.get("glob").and_then(|v| v.as_str()) {
        cmd = cmd.args(["--glob", glob]);
    }

    let output = cmd.stdout_capture().stderr_capture().run()?;

    // Exit code 2+ is an actual error
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
