use serde_json::json;

use crate::anthropic::ToolDefinition;
use crate::error::{Error, Result};
use crate::github::GitHubClient;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_pr_diff".into(),
        description: "Fetch the diff of a GitHub pull request.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "number": {
                    "type": "integer",
                    "description": "PR number"
                }
            },
            "required": ["number"]
        }),
    }
}

pub async fn execute(github: &GitHubClient, input: &serde_json::Value) -> Result<String> {
    let number = input["number"]
        .as_u64()
        .ok_or_else(|| Error::Tool("get_pr_diff: missing 'number' parameter".into()))?;

    github.get_pr_diff(number).await
}
