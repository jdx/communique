use std::path::Path;

use serde_json::json;

use crate::error::{Error, Result};
use crate::llm::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".into(),
        description:
            "Read the contents of a file in the repository. Path is relative to the repo root."
                .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to repo root"
                }
            },
            "required": ["path"]
        }),
    }
}

pub fn execute(repo_root: &Path, input: &serde_json::Value) -> Result<String> {
    let rel_path = input["path"]
        .as_str()
        .ok_or_else(|| Error::Tool("read_file: missing 'path' parameter".into()))?;

    let full_path = repo_root.join(rel_path);
    let canonical = full_path
        .canonicalize()
        .map_err(|e| Error::Tool(format!("read_file: {rel_path}: {e}")))?;

    // Sandbox: ensure resolved path is within repo root
    let root_canonical = repo_root
        .canonicalize()
        .map_err(|e| Error::Tool(format!("read_file: cannot resolve repo root: {e}")))?;
    if !canonical.starts_with(&root_canonical) {
        return Err(Error::Tool(format!(
            "read_file: path escapes repo root: {rel_path}"
        )));
    }

    let contents = xx::file::read_to_string(&canonical)
        .map_err(|e| Error::Tool(format!("read_file: {rel_path}: {e}")))?;

    // Truncate very large files
    if contents.len() > 100_000 {
        Ok(format!(
            "{}...\n\n[file truncated at 100KB]",
            &contents[..100_000]
        ))
    } else {
        Ok(contents)
    }
}
