use serde_json::json;

use crate::anthropic::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "submit_release_notes".into(),
        description: "Submit the final release notes. Call this exactly once when you are done researching and are ready to deliver the release notes.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "changelog": {
                    "type": "string",
                    "description": "Concise changelog entry using Keep a Changelog categories (### Added, ### Fixed, etc). No version header — just the categorized items."
                },
                "release_title": {
                    "type": "string",
                    "description": "A catchy, concise title for the GitHub release (no # prefix)."
                },
                "release_body": {
                    "type": "string",
                    "description": "Detailed GitHub release notes in markdown. Follow the template from the system prompt: narrative summary, optional Highlights, What's Changed, optional Breaking Changes, optional New Contributors, and a Full Changelog link."
                }
            },
            "required": ["changelog", "release_title", "release_body"]
        }),
    }
}
