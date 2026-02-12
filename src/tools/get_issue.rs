use serde_json::json;

use crate::error::{Error, Result};
use crate::github::GitHubClient;
use crate::llm::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_issue".into(),
        description: "Fetch details of a GitHub issue (title, body, labels, state, author).".into(),
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

pub async fn execute(github: &GitHubClient, input: &serde_json::Value) -> Result<String> {
    let number = input["number"]
        .as_u64()
        .ok_or_else(|| Error::Tool("get_issue: missing 'number' parameter".into()))?;

    let issue = github.get_issue(number).await?;
    Ok(format!(
        "Issue #{}: {}\nState: {}\nAuthor: @{}\nLabels: {}\n\n{}",
        issue.number,
        issue.title,
        issue.state,
        issue.user.login,
        issue
            .labels
            .iter()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        issue.body.as_deref().unwrap_or("(no description)")
    ))
}
