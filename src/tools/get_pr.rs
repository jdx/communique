use serde_json::json;

use crate::error::{Error, Result};
use crate::github::GitHubClient;
use crate::llm::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "get_pr".into(),
        description: "Fetch details of a GitHub pull request (title, body, labels, author).".into(),
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
        .ok_or_else(|| Error::Tool("get_pr: missing 'number' parameter".into()))?;

    let pr = github.get_pr(number).await?;
    Ok(format!(
        "PR #{}: {}\nAuthor: @{}\nLabels: {}\n\n{}",
        pr.number,
        pr.title,
        pr.user.login,
        pr.labels
            .iter()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        pr.body.as_deref().unwrap_or("(no description)")
    ))
}
