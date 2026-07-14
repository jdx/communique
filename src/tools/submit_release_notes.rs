use serde_json::json;

use crate::llm::ToolDefinition;

pub fn definition(include_release_notes: bool, include_changelog: bool) -> ToolDefinition {
    let mut properties = json!({});
    let mut required = Vec::new();
    if include_release_notes {
        properties["release_title"] = json!({
            "type": "string",
            "description": "A catchy, concise title for the GitHub release (no # prefix, no version tag — the version will be prepended automatically as 'vX.Y.Z: your title')."
        });
        properties["release_body"] = json!({
            "type": "string",
            "description": "Detailed GitHub release notes in markdown. Follow the template from the system prompt: narrative summary, optional Highlights only for broad releases where they synthesize themes instead of duplicating categorized bullets, categorized sections (Added, Fixed, Changed, etc.), optional Breaking Changes, optional New Contributors, and a Full Changelog link."
        });
        required.extend(["release_title", "release_body"]);
    }
    if include_changelog {
        properties["changelog"] = json!({
            "type": "string",
            "description": "Concise changelog entry using Keep a Changelog categories (## Added, ## Fixed, etc). No version header — just the categorized items. Keep this substantially shorter than the detailed release body."
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
