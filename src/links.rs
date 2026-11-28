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
/// is never closed hides the rest of the text, as it does when rendered.
fn without_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fence: Option<(char, usize)> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        let run = marker.map_or(0, |c| trimmed.chars().take_while(|&ch| ch == c).count());
        match fence {
            Some((c, len)) => {
                if marker == Some(c) && run >= len && trimmed[run..].trim().is_empty() {
                    fence = None;
                }
            }
            None if run >= 3 => fence = marker.map(|c| (c, run)),
            None => out.push_str(&without_code_spans(line)),
        }
        out.push('\n');
    }
    out
}

/// Replace each inline code span on a line with a space. A backtick run with
/// no closing run of the same length is literal text and stays.
fn without_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('`') {
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

/// How many links are requested at the same time.
const CONCURRENT_REQUESTS: usize = 8;

/// Verify all URLs in the given texts return non-404 responses.
/// Returns a list of broken URLs.
///
/// `known` holds URLs that are already known to exist, such as the pull
/// requests of the commits being released. They are not requested.
pub async fn verify(texts: &[&str], known: &[String]) -> Vec<(String, String)> {
    let mut all_urls = Vec::new();
    for text in texts {
        for url in extract_urls(text) {
            if !is_checkable(&url) {
                debug!("not checking example URL: {url}");
            } else if known.iter().any(|k| k == url.trim_end_matches('/')) {
                debug!("not checking known URL: {url}");
            } else if !all_urls.contains(&url) {
                all_urls.push(url);
            }
        }
    }

    if all_urls.is_empty() {
        return Vec::new();
    }
    // Report in a stable order, whichever request finishes first.
    all_urls.sort();

    let client = reqwest::Client::builder()
        .user_agent("communique/0.1 link-checker")
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap();

    let mut broken = Vec::new();
    for batch in all_urls.chunks(CONCURRENT_REQUESTS) {
        let checks = batch.iter().map(|url| check(&client, url));
        let reasons = futures_util::future::join_all(checks).await;
        for (url, reason) in batch.iter().zip(reasons) {
            if let Some(reason) = reason {
                warn!("broken link: {url} ({reason})");
                broken.push((url.clone(), reason));
            }
        }
    }
    broken
}

/// Request one URL and return why it is broken, if it is. Servers that do
/// not allow HEAD are asked again with GET.
async fn check(client: &reqwest::Client, url: &str) -> Option<String> {
    match client.head(url).send().await {
        Ok(resp) if resp.status() == 404 => Some("404".into()),
        Ok(resp) if resp.status() == 405 => match client.get(url).send().await {
            Ok(resp) if resp.status() == 404 => Some("404".into()),
            Ok(_) => None,
            Err(e) => Some(e.to_string()),
        },
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    }
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
        assert!(verify(&[text], &[]).await.is_empty());
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
        let broken = verify(&[&text], &[]).await;
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
        let broken = verify(&[&text], &[]).await;
        assert_eq!(broken.len(), 1);
        assert!(broken[0].1.contains("404"));
    }

    #[tokio::test]
    async fn test_verify_does_not_request_known_urls() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::any())
            .respond_with(wiremock::ResponseTemplate::new(404))
            .expect(0)
            .mount(&server)
            .await;

        let known = vec![format!("{}/o/r/pull/1", server.uri())];
        let text = format!(
            "Fixed in [#1]({0}/o/r/pull/1) and [#1 again]({0}/o/r/pull/1/).",
            server.uri()
        );
        assert!(verify(&[&text], &known).await.is_empty());
    }

    #[tokio::test]
    async fn test_verify_checks_urls_that_only_start_like_a_known_one() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let known = vec![format!("{}/o/r/pull/1", server.uri())];
        let text = format!("See {}/o/r/pull/12", server.uri());
        let broken = verify(&[&text], &known).await;
        assert_eq!(broken.len(), 1);
        assert!(broken[0].0.ends_with("/pull/12"));
    }

    #[tokio::test]
    async fn test_verify_reports_every_broken_url_in_order() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::path_regex("^/gone/"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::path_regex("^/ok/"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        // More URLs than one batch of concurrent requests.
        let mut text = String::new();
        for i in (10..30).rev() {
            text.push_str(&format!("{0}/gone/{i} {0}/ok/{i}\n", server.uri()));
        }
        let broken = verify(&[&text, &text], &[]).await;
        let expected: Vec<String> = (10..30)
            .map(|i| format!("{}/gone/{i}", server.uri()))
            .collect();
        let urls: Vec<String> = broken.into_iter().map(|(url, _)| url).collect();
        assert_eq!(urls, expected);
    }

    #[tokio::test]
    async fn test_verify_empty_text() {
        let broken = verify(&["no urls here"], &[]).await;
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
        let broken = verify(&[&text], &[]).await;
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
        let broken = verify(&[&text], &[]).await;
        assert_eq!(broken.len(), 1);
        assert!(broken[0].1.contains("404"));
    }
}
