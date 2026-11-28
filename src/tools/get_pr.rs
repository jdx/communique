use serde_json::json;

use crate::error::{Error, Result};
use crate::forge::{ForgeClient, ForgeKind};
use crate::llm::ToolDefinition;

pub fn definition(forge: ForgeKind) -> ToolDefinition {
    let (description, number) = match forge {
        ForgeKind::GitHub => (
            "Fetch details of a GitHub pull request (title, body, labels, author).",
            "PR number",
        ),
        ForgeKind::GitLab => (
            "Fetch details of a GitLab merge request (title, description, labels, author, issues it closes).",
            "Merge request number (IID), without the leading `!`",
        ),
    };
    ToolDefinition {
        name: "get_pr".into(),
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
        .ok_or_else(|| Error::Tool("get_pr: missing 'number' parameter".into()))?;

    let kind = forge.kind();
    let pr = forge.get_change_request(number).await?;
    let label = match kind {
        ForgeKind::GitHub => "PR",
        ForgeKind::GitLab => "Merge request",
    };
    let mut out = format!(
        "{label} {}: {}\nAuthor: @{}\nLabels: {}\n",
        kind.change_ref(pr.number),
        pr.title,
        pr.author,
        pr.labels.join(", "),
    );
    if !pr.closes.is_empty() {
        let issues = pr
            .closes
            .iter()
            .map(|n| format!("#{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("Closes issues: {issues}\n"));
    }
    out.push('\n');
    out.push_str(pr.body.as_deref().unwrap_or("(no description)"));
    Ok(out)
}
