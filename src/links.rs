use log::{debug, warn};
use regex::Regex;
use reqwest::Url;

/// Host names reserved for documentation and testing (RFC 2606, RFC 6761).
/// They never resolve, so a URL on one of them is an example, not a link.
const RESERVED_HOST_SUFFIXES: [&str; 7] = [
    ".example",
    ".test",
    ".invalid",
    ".localhost",
    ".example.com",
    ".example.net",
    ".example.org",
];

/// Extract the URLs a reader could follow: markdown link targets and bare
/// URLs in prose. URLs inside fenced code blocks and inline code spans are
/// left out, because there they are sample configuration or output.
fn extract_urls(text: &str) -> Vec<String> {
    let re = Regex::new(r#"https?://[^\s\)\]>"<`]+"#).unwrap();
    re.find_iter(&without_code(text))
        .map(|m| m.as_str().trim_end_matches(['.', ',', ';']).to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

/// Drop fenced code blocks and inline code spans from markdown. A fence that
/// is never closed hides the rest of its blockquote or list item, or the rest
/// of the text, as it does when rendered. Fences are found inside blockquotes
/// and list items too, and a code span may wrap onto the next line of its
/// paragraph.
fn without_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut paragraph = Paragraph::default();
    let mut fence: Option<Fence> = None;
    // The blockquote depth and content column of the latest list item, so
    // that an indented fence after it belongs to it.
    let mut item: Option<(usize, usize)> = None;
    for raw in text.lines() {
        let line = Line::parse(raw);
        if let Some(open) = &fence {
            if open.contains(&line) {
                if open.is_closed_by(raw) {
                    fence = None;
                }
                out.push('\n');
                continue;
            }
            // The blockquote or list item holding the fence ended, and the
            // fence with it. This line is ordinary Markdown again.
            fence = None;
        }
        if line.item.is_some() || line.quotes != paragraph.quotes || line.is_heading() {
            paragraph.flush(&mut out);
        }
        if let Some(col) = line.item {
            item = Some((line.quotes, col));
        } else if !line.content.is_empty()
            && item.is_some_and(|(quotes, col)| quotes != line.quotes || line.indent < col)
        {
            item = None;
        }
        if let Some((marker, len)) = fence_opener(line.content) {
            paragraph.flush(&mut out);
            let col = match (line.item, item) {
                (Some(col), _) => col,
                (None, Some((quotes, col))) if quotes == line.quotes && line.indent >= col => col,
                _ => 0,
            };
            fence = Some(Fence {
                marker,
                len,
                quotes: line.quotes,
                col,
            });
        } else if line.content.is_empty() {
            paragraph.flush(&mut out);
        } else {
            paragraph.push(raw, line.quotes);
            if line.is_heading() || line.is_underline() {
                paragraph.flush(&mut out);
            }
            continue;
        }
        out.push('\n');
    }
    paragraph.flush(&mut out);
    out
}

/// Consecutive lines of one paragraph, whose code spans can wrap between them.
#[derive(Default)]
struct Paragraph {
    text: String,
    quotes: usize,
}

impl Paragraph {
    fn push(&mut self, line: &str, quotes: usize) {
        self.text.push_str(line);
        self.text.push('\n');
        self.quotes = quotes;
    }

    fn flush(&mut self, out: &mut String) {
        out.push_str(&without_code_spans(&self.text));
        self.text.clear();
    }
}

/// An open fenced code block.
struct Fence {
    marker: char,
    len: usize,
    /// Blockquote depth of the opening line.
    quotes: usize,
    /// Content column of the list item holding the fence, or 0.
    col: usize,
}

impl Fence {
    /// Whether a line is still inside the blockquote or list item the fence
    /// opened in. A blank line does not end a list item.
    fn contains(&self, line: &Line) -> bool {
        line.quotes >= self.quotes && (line.content.is_empty() || line.indent >= self.col)
    }

    fn is_closed_by(&self, raw: &str) -> bool {
        let mut rest = raw.trim_start();
        for _ in 0..self.quotes {
            rest = rest.strip_prefix('>').unwrap_or(rest).trim_start();
        }
        fence_marker(rest).is_some_and(|(marker, run, info)| {
            marker == self.marker && run >= self.len && info.trim().is_empty()
        })
    }
}

/// A line split into its Markdown containers. Columns are byte offsets into
/// the line.
struct Line<'a> {
    /// Number of blockquote markers (`>`).
    quotes: usize,
    /// Column of the first character after the leading blockquote markers.
    indent: usize,
    /// Content column of the list item (`-`, `*`, `+`, `1.`, `1)`) this line
    /// opens.
    item: Option<usize>,
    /// The text inside the containers.
    content: &'a str,
}

impl<'a> Line<'a> {
    fn parse(line: &'a str) -> Self {
        let column = |rest: &str| line.len() - rest.len();
        let mut quotes = 0;
        let mut rest = line.trim_start();
        while let Some(after) = rest.strip_prefix('>') {
            quotes += 1;
            rest = after.trim_start();
        }
        let indent = column(rest);
        let mut item = None;
        loop {
            if let Some(after) = rest.strip_prefix('>') {
                quotes += 1;
                rest = after.trim_start();
            } else if let Some(after) = after_list_marker(rest) {
                rest = after.trim_start();
                item = Some(column(rest));
            } else {
                break;
            }
        }
        Self {
            quotes,
            indent,
            item,
            content: rest.trim_end(),
        }
    }

    /// A setext heading underline (`===`, `---`) or a thematic break (`***`,
    /// `___`). Either one ends the paragraph above it.
    fn is_underline(&self) -> bool {
        let mut chars = self.content.chars().filter(|c| !c.is_whitespace());
        chars.next().is_some_and(|first| {
            matches!(first, '=' | '-' | '*' | '_') && chars.all(|c| c == first)
        })
    }

    fn is_heading(&self) -> bool {
        let hashes = self.content.bytes().take_while(|&b| b == b'#').count();
        (1..=6).contains(&hashes)
            && self.content[hashes..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
    }
}

fn after_list_marker(text: &str) -> Option<&str> {
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    let len = match (digits, text.as_bytes().get(digits)) {
        (1..=9, Some(b'.' | b')')) => digits + 1,
        (0, Some(b'-' | b'*' | b'+')) => 1,
        _ => return None,
    };
    let after = &text[len..];
    (after.is_empty() || after.starts_with([' ', '\t'])).then_some(after)
}

/// The marker character, run length and info string of a line that starts
/// with three or more backticks or tildes.
fn fence_marker(content: &str) -> Option<(char, usize, &str)> {
    let c = content.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let run = content.bytes().take_while(|&b| b == c as u8).count();
    (run >= 3).then(|| (c, run, &content[run..]))
}

/// The fence a line opens. A backtick fence cannot have a backtick in its
/// info string, so a line such as "```cmd``` first" is an inline code span.
fn fence_opener(content: &str) -> Option<(char, usize)> {
    let (c, run, info) = fence_marker(content)?;
    (c == '~' || !info.contains('`')).then_some((c, run))
}

/// Replace each inline code span in a paragraph with a space. A backtick run
/// with no closing run of the same length is literal text and stays, and so
/// does a backtick escaped with a backslash.
fn without_code_spans(paragraph: &str) -> String {
    let mut out = String::with_capacity(paragraph.len());
    let mut rest = paragraph;
    while let Some(start) = opening_backtick(rest) {
        let run = backtick_run(&rest[start..]);
        let after = &rest[start + run..];
        match closing_run(after, run) {
            Some(end) => {
                out.push_str(&rest[..start]);
                out.push(' ');
                rest = &after[end + run..];
            }
            None => {
                out.push_str(&rest[..start + run]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Offset of the first backtick that a backslash does not escape. Escapes do
/// not apply inside a code span, so only the opening run is searched this way.
fn opening_backtick(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'`' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn backtick_run(text: &str) -> usize {
    text.bytes().take_while(|&b| b == b'`').count()
}

/// Offset of the next backtick run that is exactly `run` long.
fn closing_run(text: &str, run: usize) -> Option<usize> {
    let mut offset = 0;
    while let Some(found) = text[offset..].find('`') {
        let start = offset + found;
        let len = backtick_run(&text[start..]);
        if len == run {
            return Some(start);
        }
        offset = start + len;
    }
    None
}

/// Whether `url` can be requested: it parses, and its host is not a reserved
/// example name. A template such as `http://127.0.0.1:{{ port }}` fails here.
fn is_checkable(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    !RESERVED_HOST_SUFFIXES
        .iter()
        .any(|suffix| host == suffix[1..] || host.ends_with(suffix))
}

/// Verify all URLs in the given texts return non-404 responses.
/// Returns a list of broken URLs.
pub async fn verify(texts: &[&str]) -> Vec<(String, String)> {
    let mut all_urls = Vec::new();
    for text in texts {
        for url in extract_urls(text) {
            if is_checkable(&url) {
                all_urls.push(url);
            } else {
                debug!("not checking example URL: {url}");
            }
        }
    }

    if all_urls.is_empty() {
        return Vec::new();
    }

    let client = reqwest::Client::builder()
        .user_agent("communique/0.1 link-checker")
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap();

    let mut broken = Vec::new();
    for url in &all_urls {
        match client.head(url).send().await {
            Ok(resp) if resp.status() == 404 => {
                warn!("broken link: {url} (404)");
                broken.push((url.clone(), "404".into()));
            }
            Ok(resp) if resp.status() == 405 => {
                // HEAD not allowed, try GET
                match client.get(url).send().await {
                    Ok(resp) if resp.status() == 404 => {
                        warn!("broken link: {url} (404)");
                        broken.push((url.clone(), "404".into()));
                    }
                    Err(e) => {
                        warn!("broken link: {url} ({e})");
                        broken.push((url.clone(), e.to_string()));
                    }
                    _ => {}
                }
            }
            Err(e) => {
                warn!("broken link: {url} ({e})");
                broken.push((url.clone(), e.to_string()));
            }
            _ => {}
        }
    }
    broken
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_urls() {
        let text =
            "Check [docs](https://example.com/docs) and https://github.com/jdx/communique/pull/1.";
        let urls = extract_urls(text);
        assert!(urls.contains(&"https://example.com/docs".to_string()));
        assert!(urls.contains(&"https://github.com/jdx/communique/pull/1".to_string()));
    }

    #[test]
    fn test_extract_urls_dedup() {
        let text = "See https://example.com and https://example.com again.";
        let urls = extract_urls(text);
        assert_eq!(urls.len(), 1);
    }

    #[test]
    fn test_extract_urls_skips_fenced_code() {
        let text = "Proxy settings can now be templated:\n\n\
            ```toml\n[settings]\nurl = \"http://127.0.0.1:{{ port }}\"\n```\n\n\
            ~~~~sh\ncurl https://tilde.test/a\n~~~\nstill https://tilde.test/b\n~~~~\n\n\
            See [the docs](https://docs.rs/regex).\n";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/regex"]);
    }

    #[test]
    fn test_extract_urls_skips_indented_fence() {
        let text = "- A list item:\n\n  ```sh\n  curl https://inside.test/x\n  ```\n\n  \
            and https://docs.rs/after\n";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/after"]);
    }

    #[test]
    fn test_extract_urls_skips_code_spans() {
        let text = "Set `index-url = \"https://gitlab.example.com/api/v4/simple\"` or \
            ``a ` https://double.test/x`` and read https://docs.rs/reqwest.";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/reqwest"]);
    }

    #[test]
    fn test_extract_urls_keeps_text_after_unclosed_backtick() {
        let text = "A stray ` backtick, then https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_inline_triple_backticks_are_not_a_fence() {
        let text = "Run ```cmd``` first.\nThen read https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_skips_code_spans_that_wrap() {
        let text = "Run `curl\nhttp://127.0.0.1:1/sample` now.\nThen read https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_code_spans_end_at_a_blank_line() {
        let text = "A stray `\n\nhttps://docs.rs/tokio` here";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_skips_fences_in_blockquotes() {
        let text = "> [!NOTE]\n> ~~~sh\n> curl http://127.0.0.1:1/sample\n> ~~~\n> See https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_skips_fences_in_list_items() {
        let text = "- ```sh\n  curl http://127.0.0.1:1/sample\n  ```\n1. See https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_ends_a_quoted_fence_with_its_quote() {
        let text = "> ~~~sh\n> curl http://127.0.0.1:1/sample\n\nSee https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_ends_a_list_fence_with_its_item() {
        let text = "- ```sh\n  curl http://127.0.0.1:1/sample\n\nSee https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
        let text = "- step\n\n  ```sh\n  curl http://127.0.0.1:1/sample\n\n  still code\nSee https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_keeps_code_spans_within_a_list_item() {
        let text =
            "- Handle a literal ` character (https://docs.rs/tokio).\n- Use `config` for settings.";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
        let text = "# A ` heading\nSee https://docs.rs/tokio and `code`";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
        let text = "A stray `\n## Links\n[docs](https://docs.rs/tokio)`";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
        for underline in ["===", "---", "***"] {
            let text = format!("A stray `\n{underline}\n[docs](https://docs.rs/tokio)`");
            assert_eq!(
                extract_urls(&text),
                vec!["https://docs.rs/tokio"],
                "{underline}"
            );
        }
        let text = "Run `mise\n=== http://127.0.0.1:1/sample` and see https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_reads_list_markers_in_a_fence_as_code() {
        let text = "~~~sh\n- ~~~\nhttp://127.0.0.1:1/sample\n~~~\nSee https://docs.rs/tokio";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_keeps_links_between_escaped_backticks() {
        let text = r"Read \`[the docs](https://docs.rs/tokio)\` first";
        assert_eq!(extract_urls(text), vec!["https://docs.rs/tokio"]);
    }

    #[test]
    fn test_extract_urls_stops_at_quote_and_angle_bracket() {
        let text = "<a href=\"https://docs.rs/a\">a</a> and \"https://docs.rs/b\"";
        let mut urls = extract_urls(text);
        urls.sort();
        assert_eq!(urls, vec!["https://docs.rs/a", "https://docs.rs/b"]);
    }

    #[test]
    fn test_is_checkable() {
        for url in [
            "https://github.com/jdx/communique/pull/1",
            "http://127.0.0.1:8080/page",
            "https://docs.rs/regex",
            "https://myexample.com/",
        ] {
            assert!(is_checkable(url), "{url} should be checked");
        }
        for url in [
            "http://127.0.0.1:{{",
            "https://example.com/docs",
            "https://gitlab.example.com/api/v4/simple",
            "https://EXAMPLE.org./x",
            "https://registry.example/x",
            "https://mirror.test/x",
            "https://nothing.invalid/",
            "http://localhost:3000/",
            "http://app.localhost/",
        ] {
            assert!(!is_checkable(url), "{url} should not be checked");
        }
    }

    #[tokio::test]
    async fn test_verify_ignores_examples() {
        // None of these can be fetched, so reporting them would make the
        // model delete a valid example from the notes.
        let text = "Templated: http://127.0.0.1:{{ port }} and \
            https://gitlab.example.com/api/v4/simple\n\n\
            ```toml\nurl = \"http://127.0.0.1:1/unreachable\"\n```\n";
        assert!(verify(&[text]).await.is_empty());
    }

    #[tokio::test]
    async fn test_verify_all_ok() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let url = format!("{}/page", server.uri());
        let text = format!("Check {url}");
        let broken = verify(&[&text]).await;
        assert!(broken.is_empty());
    }

    #[tokio::test]
    async fn test_verify_broken_404() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let url = format!("{}/broken", server.uri());
        let text = format!("See {url}");
        let broken = verify(&[&text]).await;
        assert_eq!(broken.len(), 1);
        assert!(broken[0].1.contains("404"));
    }

    #[tokio::test]
    async fn test_verify_empty_text() {
        let broken = verify(&["no urls here"]).await;
        assert!(broken.is_empty());
    }

    #[tokio::test]
    async fn test_verify_405_fallback_to_get_ok() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .respond_with(wiremock::ResponseTemplate::new(405))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let url = format!("{}/page", server.uri());
        let text = format!("Check {url}");
        let broken = verify(&[&text]).await;
        assert!(broken.is_empty());
    }

    #[tokio::test]
    async fn test_verify_405_fallback_to_get_404() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .respond_with(wiremock::ResponseTemplate::new(405))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let url = format!("{}/broken", server.uri());
        let text = format!("See {url}");
        let broken = verify(&[&text]).await;
        assert_eq!(broken.len(), 1);
        assert!(broken[0].1.contains("404"));
    }
}
