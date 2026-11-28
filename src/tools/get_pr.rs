use serde_json::json;

use crate::error::{Error, Result};
use crate::forge::{ChangeRequest, ForgeClient, ForgeKind};
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

    let pr = forge.get_change_request(number).await?;
    Ok(render(forge.kind(), &pr, None))
}

/// The text the model reads for a pull or merge request. With `max_body`,
/// a longer description is cut there and marked as truncated.
pub fn render(kind: ForgeKind, pr: &ChangeRequest, max_body: Option<usize>) -> String {
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
        let issues = pr.closes.join(", ");
        out.push_str(&format!("Closes issues: {issues}\n"));
    }
    out.push('\n');
    let body = pr.body.as_deref().unwrap_or("(no description)");
    match max_body {
        Some(max) if body.len() > max => {
            let mut end = max;
            while !body.is_char_boundary(end) {
                end -= 1;
            }
            out.push_str(&body[..end]);
            out.push_str("...\n[description truncated: call get_pr for the rest]");
        }
        _ => out.push_str(body),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change_request(body: Option<&str>) -> ChangeRequest {
        ChangeRequest {
            number: 42,
            title: "Add feature".into(),
            body: body.map(str::to_string),
            author: "octocat".into(),
            labels: vec!["enhancement".into(), "cli".into()],
            closes: vec![],
        }
    }

    #[test]
    fn render_shows_the_whole_description_by_default() {
        let pr = change_request(Some("Adds `--fast`."));
        assert_eq!(
            render(ForgeKind::GitHub, &pr, None),
            "PR #42: Add feature\nAuthor: @octocat\nLabels: enhancement, cli\n\nAdds `--fast`."
        );
        assert!(
            render(ForgeKind::GitHub, &change_request(None), None).ends_with("(no description)")
        );
    }

    #[test]
    fn render_names_closed_issues_on_gitlab() {
        let mut pr = change_request(Some("Body"));
        pr.closes = vec!["#7".into()];
        let out = render(ForgeKind::GitLab, &pr, None);
        assert!(out.starts_with("Merge request !42: Add feature\n"));
        assert!(out.contains("Closes issues: #7\n"));
    }

    #[test]
    fn render_cuts_a_long_description_on_a_character_boundary() {
        let pr = change_request(Some("ééééé"));
        let out = render(ForgeKind::GitHub, &pr, Some(5));
        assert!(out.ends_with("\n\néé...\n[description truncated: call get_pr for the rest]"));
        let whole = render(ForgeKind::GitHub, &pr, Some(10));
        assert!(whole.ends_with("ééééé"));
    }
}
