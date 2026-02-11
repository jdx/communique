use crate::error::{Error, Result};

pub struct ParsedOutput {
    pub changelog: String,
    pub release_title: String,
    pub release_body: String,
}

pub fn parse(raw: &str) -> Result<ParsedOutput> {
    let parts: Vec<&str> = raw.splitn(2, "---SECTION_BREAK---").collect();
    if parts.len() != 2 {
        return Err(Error::Parse(
            "expected two sections separated by ---SECTION_BREAK---".into(),
        ));
    }

    let changelog = parts[0].trim().to_string();
    let release_raw = parts[1].trim();

    // Extract title from first H1 line
    let (title, body) = if let Some(rest) = release_raw.strip_prefix("# ") {
        match rest.find('\n') {
            Some(pos) => (rest[..pos].trim().to_string(), rest[pos + 1..].trim().to_string()),
            None => (rest.trim().to_string(), String::new()),
        }
    } else {
        // No H1 found — use tag as title
        ("Release".to_string(), release_raw.to_string())
    };

    Ok(ParsedOutput {
        changelog,
        release_title: title,
        release_body: body,
    })
}
