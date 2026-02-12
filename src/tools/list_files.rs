use std::path::Path;

use serde_json::json;
use xx::process;

use crate::error::Result;
use crate::llm::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "list_files".into(),
        description:
            "List files tracked by git in the repository. Optionally filter by a glob pattern."
                .into(),
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
    let mut cmd = process::cmd("git", ["ls-files"]).cwd(repo_root);

    if let Some(pattern) = input.get("pattern").and_then(|v| v.as_str()) {
        cmd = cmd.args(["--", pattern]);
    }

    Ok(cmd.read()?)
}
