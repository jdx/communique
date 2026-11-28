use serde_json::json;

use crate::error::{Error, Result};
use crate::forge::{ForgeClient, ForgeKind};
use crate::llm::ToolDefinition;

pub fn definition(forge: ForgeKind) -> ToolDefinition {
    let (description, number) = match forge {
        ForgeKind::GitHub => ("Fetch the diff of a GitHub pull request.", "PR number"),
        ForgeKind::GitLab => (
            "Fetch the diff of a GitLab merge request.",
            "Merge request number (IID), without the leading `!`",
        ),
    };
    ToolDefinition {
        name: "get_pr_diff".into(),
        description: description.into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "number": {
                    "type": "integer",
                    "description": number
                }
            },
            "required": ["number"]
        }),
    }
}

pub async fn execute(forge: &ForgeClient, input: &serde_json::Value) -> Result<String> {
    let number = input["number"]
        .as_u64()
        .ok_or_else(|| Error::Tool("get_pr_diff: missing 'number' parameter".into()))?;

    forge.get_change_request_diff(number).await
}
