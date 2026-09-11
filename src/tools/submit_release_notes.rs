use serde_json::json;

use crate::llm::ToolDefinition;

pub fn definition(include_release_notes: bool, include_changelog: bool) -> ToolDefinition {
    let mut properties = json!({                "coverage": {
                    "type": "array",
                    "description": "When requested, assess every supplied commit against the final notes with an exact commit ID, status, and reason.",
                    "items": {"type": "object", "properties": {
                        "commit": {"type": "string"},
                        "status": {"type": "string", "enum": ["included", "omitted", "uncertain"]},
                        "reason": {"type": "string"}
                    }, "required": ["commit", "status", "reason"]}
                },
                "migration_guide": {"type": "string", "description": "When requested, standalone Markdown upgrade steps, affected users, and verified before/after examples. Explicitly state when no migration is needed."}});
    let mut required = Vec::new();
    if include_release_notes {
        properties["release_title"] = json!({
            "type": "string",
            "description": "A concise, concrete title naming the main user-visible change, or 'Maintenance release' when there are no user-facing changes, for the GitHub release (no # prefix, no version tag — the version will be prepended automatically as 'vX.Y.Z: your title')."
        });
        properties["release_body"] = json!({
            "type": "string",
            "description": "GitHub release notes in markdown following the editorial guidelines in the system prompt. Scale the length and sections to user impact, explain each change once, and preserve essential examples and upgrade instructions. For maintenance-only releases, use one sentence and the Full Changelog link. Reference material supplies terminology and formatting, not requirements to copy its structure or footers."
        });
        required.extend(["release_title", "release_body"]);
    }
    if include_changelog {
        properties["changelog"] = json!({
            "type": "string",
            "description": "Concise changelog entry using Keep a Changelog categories (## Added, ## Fixed, etc). No version header — just the categorized items. When detailed release notes are also requested, keep this substantially shorter. For maintenance-only releases, use ## Changed followed by a single bullet: No user-facing changes. Omit a narrative introduction and Full Changelog link."
        });
        required.push("changelog");
    }

    ToolDefinition {
        name: "submit_release_notes".into(),
        description: "Submit the final release notes. Call this exactly once when you are done researching and are ready to deliver the release notes.".into(),
        input_schema: json!({
            "type": "object",
            "properties": properties,
            "required": required
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detailed_only_schema_omits_changelog() {
        let definition = definition(true, false);
        assert!(definition.input_schema["properties"]["changelog"].is_null());
        assert_eq!(
            definition.input_schema["required"],
            json!(["release_title", "release_body"])
        );
    }

    #[test]
    fn combined_schema_requires_changelog() {
        let definition = definition(true, true);
        assert!(definition.input_schema["properties"]["changelog"].is_object());
        assert_eq!(
            definition.input_schema["required"],
            json!(["release_title", "release_body", "changelog"])
        );
    }

    #[test]
    fn changelog_only_schema_omits_release_notes() {
        let definition = definition(false, true);
        assert!(definition.input_schema["properties"]["release_title"].is_null());
        assert!(definition.input_schema["properties"]["release_body"].is_null());
        assert_eq!(definition.input_schema["required"], json!(["changelog"]));
    }
}
