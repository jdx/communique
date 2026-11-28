use serde_json::json;

use crate::error::{Error, Result};
use crate::forge::{ForgeClient, ForgeKind};
use crate::llm::ToolDefinition;

pub fn definition(forge: ForgeKind) -> ToolDefinition {
    ToolDefinition {
        name: "get_issue".into(),
        description: format!(
            "Fetch details of a {} issue (title, body, labels, state, author).",
            forge.name()
        ),
        input_schema: json!({
            "type": "object",
            "properties": {
                "number": {
                    "type": "integer",
                    "description": "Issue number"
                }
            },
            "required": ["number"]
        }),
    }
}

pub async fn execute(forge: &ForgeClient, input: &serde_json::Value) -> Result<String> {
    let number = input["number"]
        .as_u64()
        .ok_or_else(|| Error::Tool("get_issue: missing 'number' parameter".into()))?;

    let issue = forge.get_issue(number).await?;
    Ok(format!(
        "Issue #{}: {}\nState: {}\nAuthor: @{}\nLabels: {}\n\n{}",
        issue.number,
        issue.title,
        issue.state,
        issue.author,
        issue.labels.join(", "),
        issue.body.as_deref().unwrap_or("(no description)")
    ))
}
