use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::anthropic::ToolDefinition;
use crate::error::{Error, Result};

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "list_files".into(),
        description: "List files tracked by git in the repository. Optionally filter by a glob pattern.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Optional glob pattern to filter files (e.g. 'src/**/*.rs')"
                }
            }
        }),
    }
}

pub fn execute(repo_root: &Path, input: &serde_json::Value) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("ls-files").current_dir(repo_root);

    if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
        cmd.arg("--").arg(pattern);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        return Err(Error::Tool(format!(
            "list_files: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
