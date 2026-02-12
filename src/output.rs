#[derive(Debug)]
pub struct ParsedOutput {
    pub changelog: String,
    pub release_title: String,
    pub release_body: String,
}

/// Attempt to parse raw text from the LLM into a ParsedOutput.
///
/// Used as a fallback when the model returns text instead of calling
/// `submit_release_notes`. Extracts a `# Title` heading as the release title
/// and uses the remaining text as both the release body and changelog.
pub fn parse_text_fallback(text: &str) -> Option<ParsedOutput> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    // Try to extract a leading "# Title" heading
    if let Some(rest) = text.strip_prefix("# ")
        && let Some(newline_pos) = rest.find('\n')
    {
        let title = rest[..newline_pos].trim().to_string();
        let body = rest[newline_pos + 1..].trim().to_string();
        if !body.is_empty() {
            return Some(ParsedOutput {
                changelog: body.clone(),
                release_title: title,
                release_body: body,
            });
        }
    }

    // No heading found — use entire text as body, derive title from first line
    let first_line = text.lines().next().unwrap_or(text);
    let title = first_line
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(80)
        .collect::<String>();

    Some(ParsedOutput {
        changelog: text.to_string(),
        release_title: title,
        release_body: text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_empty() {
        assert!(parse_text_fallback("").is_none());
        assert!(parse_text_fallback("  \n  ").is_none());
    }

    #[test]
    fn test_fallback_with_heading() {
        let text = "# Great Release\n\nSome **cool** changes\n- Added feature X";
        let parsed = parse_text_fallback(text).unwrap();
        assert_eq!(parsed.release_title, "Great Release");
        assert_eq!(
            parsed.release_body,
            "Some **cool** changes\n- Added feature X"
        );
        assert_eq!(parsed.changelog, parsed.release_body);
    }

    #[test]
    fn test_fallback_heading_only() {
        // Heading with no body — falls through to the no-heading branch
        let text = "# Just a Title";
        let parsed = parse_text_fallback(text).unwrap();
        assert_eq!(parsed.release_title, "Just a Title");
        assert_eq!(parsed.release_body, text);
    }

    #[test]
    fn test_fallback_no_heading() {
        let text = "Some release notes\n\nWith multiple paragraphs";
        let parsed = parse_text_fallback(text).unwrap();
        assert_eq!(parsed.release_title, "Some release notes");
        assert_eq!(parsed.release_body, text);
    }
}
