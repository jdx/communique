use std::path::Path;
use std::process::Command;

use serde_json::json;

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

    let mut cmd = Command::new("rg");
    cmd.args(["--line-number", "--no-heading", "--max-count", "50"])
        .arg(pattern)
        .current_dir(repo_root);

    if let Some(glob) = input.get("glob").and_then(|v| v.as_str()) {
        cmd.args(["--glob", glob]);
    }

    let output = cmd.output().map_err(|e| {
        Error::Tool(format!(
            "grep: failed to run rg (is ripgrep installed?): {e}"
        ))
    })?;

    // rg returns exit code 1 for no matches — that's not an error
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(Error::Tool(format!(
            "grep: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let result = String::from_utf8_lossy(&output.stdout).to_string();
    if result.is_empty() {
        Ok("No matches found.".into())
    } else {
        Ok(result)
    }
}
