use log::warn;
use regex::Regex;
use reqwest::{StatusCode, Url};

const DEFAULT_GITHUB_API_BASE_URL: &str = "https://api.github.com";

pub(crate) struct VerifyOptions<'a> {
    github_token: Option<&'a str>,
    github_api_base_url: Url,
}

impl<'a> Default for VerifyOptions<'a> {
    fn default() -> Self {
        Self {
            github_token: None,
            github_api_base_url: Url::parse(DEFAULT_GITHUB_API_BASE_URL)
                .expect("default GitHub API base URL must be valid"),
        }
    }
}

impl<'a> VerifyOptions<'a> {
    pub(crate) fn new(github_token: Option<&'a str>, mut github_api_base_url: Url) -> Self {
        assert!(
            github_api_base_url.host_str().is_some(),
            "GitHub API base URL must include a host"
        );
        // Query strings and fragments are not meaningful for an API base URL.
        github_api_base_url.set_query(None);
        github_api_base_url.set_fragment(None);
        Self {
            github_token,
            github_api_base_url,
        }
    }
}

/// Extract all markdown links and bare URLs from text.
fn extract_urls(text: &str) -> Vec<String> {
    let re = Regex::new(r"https?://[^\s\)\]>]+").unwrap();
    re.find_iter(text)
        .map(|m| m.as_str().trim_end_matches(['.', ',', ';']).to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect()
}

/// Verify URLs in the given texts and report inaccessible links.
///
/// Generic URLs preserve the historical behavior of only reporting `404` responses
/// and network errors. Exact GitHub URLs may also report `401`/`403` responses so
/// private or unauthorized links can be surfaced with more actionable diagnostics.
pub async fn verify(texts: &[&str]) -> Vec<(String, String)> {
    verify_with_options(texts, &VerifyOptions::default()).await
}

/// Verify all URLs in the given texts with optional GitHub API authentication.
///
/// GitHub bearer auth is only sent to exact GitHub API URLs or to the configured
/// GitHub API base URL after recognized `github.com/{owner}/{repo}/...` browser
/// URLs are translated to API endpoints.
pub(crate) async fn verify_with_options(
    texts: &[&str],
    options: &VerifyOptions<'_>,
) -> Vec<(String, String)> {
    let mut all_urls = Vec::new();
    for text in texts {
        all_urls.extend(extract_urls(text));
    }

    if all_urls.is_empty() {
        return Vec::new();
    }

    let generic_client = reqwest::Client::builder()
        .user_agent("communique/0.1 link-checker")
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap();
    let github_api_client = reqwest::Client::builder()
        .user_agent("communique/0.1 link-checker")
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let github_token = options.github_token.filter(|token| !token.is_empty());
    let mut broken = Vec::new();
    for url in &all_urls {
        let parsed = Url::parse(url).ok();
        let github_api_request = github_token.and_then(|token| {
            parsed
                .as_ref()
                .and_then(|parsed_url| {
                    github_api_path_for(parsed_url, &options.github_api_base_url)
                })
                .map(|api_path| (token, api_path))
        });
        if let Some((token, api_path)) = github_api_request {
            if let Some(reason) = verify_github_api(
                &github_api_client,
                token,
                &options.github_api_base_url,
                &api_path,
            )
            .await
            {
                warn!("broken link: {url} ({reason})");
                broken.push((url.clone(), reason));
            }
            continue;
        }

        let is_github_url = parsed.as_ref().is_some_and(is_exact_github_url);
        if let Some(reason) =
            verify_generic(&generic_client, url, is_github_url, github_token.is_some()).await
        {
            warn!("broken link: {url} ({reason})");
            broken.push((url.clone(), reason));
        }
    }
    broken
}

async fn verify_generic(
    client: &reqwest::Client,
    url: &str,
    is_github_url: bool,
    github_token_available: bool,
) -> Option<String> {
    match client.head(url).send().await {
        Ok(resp) if should_report_status(resp.status(), is_github_url) => Some(status_reason(
            resp.status(),
            is_github_url,
            github_token_available,
        )),
        Ok(resp) if resp.status() == StatusCode::METHOD_NOT_ALLOWED => {
            // HEAD not allowed, try GET
            match client.get(url).send().await {
                Ok(resp) if should_report_status(resp.status(), is_github_url) => Some(
                    status_reason(resp.status(), is_github_url, github_token_available),
                ),
                Err(e) => Some(e.to_string()),
                _ => None,
            }
        }
        Err(e) => Some(e.to_string()),
        _ => None,
    }
}

async fn verify_github_api(
    client: &reqwest::Client,
    token: &str,
    api_base_url: &Url,
    api_path: &str,
) -> Option<String> {
    debug_assert!(!token.is_empty());
    let api_url = match append_api_path(api_base_url, api_path) {
        Ok(url) => url,
        Err(reason) => return Some(reason),
    };
    match client
        .get(api_url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => None,
        Ok(resp) => Some(github_api_status_reason(resp.status())),
        Err(e) => Some(e.to_string()),
    }
}

fn append_api_path(api_base_url: &Url, api_path: &str) -> Result<Url, String> {
    debug_assert!(api_path.starts_with('/'));
    let base = api_base_url.as_str().trim_end_matches('/');
    Url::parse(&format!("{base}{api_path}"))
        .map_err(|e| format!("invalid GitHub API URL for {api_path}: {e}"))
}

fn should_report_status(status: StatusCode, is_github_url: bool) -> bool {
    status == StatusCode::NOT_FOUND
        || (is_github_url && matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN))
}

fn status_reason(status: StatusCode, is_github_url: bool, github_token_available: bool) -> String {
    if is_github_url {
        github_browser_status_reason(status, github_token_available)
    } else {
        status.as_u16().to_string()
    }
}

fn github_browser_status_reason(status: StatusCode, github_token_available: bool) -> String {
    let code = status.as_u16();
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND
            if github_token_available =>
        {
            format!(
                "{code} (GitHub link shape is not API-verifiable; checked without authentication)"
            )
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => format!(
            "{code} (GitHub link is inaccessible or unverified; it may require GITHUB_TOKEN access)"
        ),
        _ => code.to_string(),
    }
}

fn github_api_status_reason(status: StatusCode) -> String {
    let code = status.as_u16();
    match status {
        StatusCode::UNAUTHORIZED => format!("{code} (GitHub token was rejected)"),
        StatusCode::FORBIDDEN => {
            format!("{code} (GitHub token may not have sufficient permissions)")
        }
        StatusCode::NOT_FOUND => format!("{code} (GitHub link not found or token lacks access)"),
        _ => code.to_string(),
    }
}

fn is_exact_github_url(url: &Url) -> bool {
    is_exact_https_host(url, "github.com") || is_exact_https_host(url, "api.github.com")
}

fn is_exact_https_host(url: &Url, host: &str) -> bool {
    url.scheme() == "https" && url.host_str() == Some(host)
}

fn github_api_path_for(url: &Url, api_base_url: &Url) -> Option<String> {
    if is_exact_https_host(url, "github.com") {
        return browser_url_to_api_path(url);
    }
    if is_exact_https_host(url, "api.github.com") {
        return Some(url.path().to_string());
    }
    if is_under_configured_api_base(url, api_base_url) {
        return configured_api_path(url, api_base_url);
    }
    None
}

fn browser_url_to_api_path(url: &Url) -> Option<String> {
    debug_assert!(is_exact_https_host(url, "github.com"));
    let segments: Vec<_> = url
        .path()
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.len() < 2 {
        return None;
    }

    let owner = segments[0];
    let repo = segments[1];
    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    match segments.get(2).copied() {
        None => Some(repo_api_path(owner, repo, "")),
        Some("pull") if segments.len() == 4 => {
            let number = issue_number(segments[3])?;
            Some(repo_api_path(owner, repo, &format!("/pulls/{number}")))
        }
        Some("issues") if segments.len() == 4 => {
            let number = issue_number(segments[3])?;
            Some(repo_api_path(owner, repo, &format!("/issues/{number}")))
        }
        Some("commit") if segments.len() == 4 => Some(repo_api_path(
            owner,
            repo,
            &format!("/commits/{}", segments[3]),
        )),
        Some("commits") if segments.len() == 4 => Some(repo_api_path(
            owner,
            repo,
            &format!("/commits/{}", segments[3]),
        )),
        Some("releases") => release_api_path(owner, repo, &segments),
        Some("compare") if segments.len() >= 4 => {
            let compare = segments[3..].join("/");
            Some(repo_api_path(owner, repo, &format!("/compare/{compare}")))
        }
        _ => None,
    }
}

fn release_api_path(owner: &str, repo: &str, segments: &[&str]) -> Option<String> {
    match segments.get(3).copied() {
        None if segments.len() == 3 => Some(repo_api_path(owner, repo, "/releases")),
        Some("tag") if segments.len() >= 5 => {
            let tag = segments[4..].join("/");
            Some(repo_api_path(owner, repo, &format!("/releases/tags/{tag}")))
        }
        Some("latest") if segments.len() == 4 => {
            Some(repo_api_path(owner, repo, "/releases/latest"))
        }
        _ => None,
    }
}

fn issue_number(segment: &str) -> Option<u64> {
    segment.parse::<u64>().ok().filter(|number| *number > 0)
}

fn repo_api_path(owner: &str, repo: &str, suffix: &str) -> String {
    format!("/repos/{owner}/{repo}{suffix}")
}

fn is_under_configured_api_base(url: &Url, api_base_url: &Url) -> bool {
    url.scheme() == api_base_url.scheme()
        && url.host_str() == api_base_url.host_str()
        && url.port_or_known_default() == api_base_url.port_or_known_default()
        && configured_api_path(url, api_base_url).is_some()
}

fn configured_api_path(url: &Url, api_base_url: &Url) -> Option<String> {
    let url_path = url.path();
    let base_path = api_base_url.path().trim_end_matches('/');
    if base_path.is_empty() {
        return Some(url_path.to_string());
    }
    if url_path == base_path {
        return Some("/".into());
    }
    url_path
        .strip_prefix(&format!("{base_path}/"))
        .map(|rest| format!("/{rest}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn url(input: &str) -> Url {
        Url::parse(input).unwrap()
    }

    fn options_with_github_api(server: &MockServer) -> VerifyOptions<'static> {
        VerifyOptions::new(Some("test-token"), Url::parse(&server.uri()).unwrap())
    }

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

    #[tokio::test]
    async fn test_verify_all_ok() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let url = format!("{}/page", server.uri());
        let text = format!("Check {url}");
        let broken = verify(&[&text]).await;
        assert!(broken.is_empty());
    }

    #[tokio::test]
    async fn test_verify_broken_404() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(404))
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
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(405))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let url = format!("{}/page", server.uri());
        let text = format!("Check {url}");
        let broken = verify(&[&text]).await;
        assert!(broken.is_empty());
    }

    #[tokio::test]
    async fn test_verify_405_fallback_to_get_404() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(405))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let url = format!("{}/broken", server.uri());
        let text = format!("See {url}");
        let broken = verify(&[&text]).await;
        assert_eq!(broken.len(), 1);
        assert!(broken[0].1.contains("404"));
    }

    #[test]
    fn test_github_classifier_accepts_exact_hosts() {
        let options = VerifyOptions::default();
        assert_eq!(
            github_api_path_for(
                &url("https://github.com/owner/repo/pull/7"),
                &options.github_api_base_url
            ),
            Some("/repos/owner/repo/pulls/7".into())
        );
        assert_eq!(
            github_api_path_for(
                &url("https://api.github.com/repos/owner/repo/issues/7"),
                &options.github_api_base_url
            ),
            Some("/repos/owner/repo/issues/7".into())
        );
    }

    #[test]
    fn test_github_classifier_rejects_spoofed_hosts() {
        let options = VerifyOptions::default();
        for input in [
            "http://github.com/owner/repo/pull/7",
            "https://github.com.evil.com/owner/repo/pull/7",
            "https://owner.github.io/repo/pull/7",
            "https://raw.githubusercontent.com/owner/repo/main/README.md",
        ] {
            assert_eq!(
                github_api_path_for(&url(input), &options.github_api_base_url),
                None,
                "{input} must not be classified as GitHub API-verifiable"
            );
        }
    }

    #[test]
    fn test_github_browser_urls_map_to_api_paths() {
        let options = VerifyOptions::default();
        for (input, expected) in [
            ("https://github.com/owner/repo", "/repos/owner/repo"),
            (
                "https://github.com/owner/repo/pull/123",
                "/repos/owner/repo/pulls/123",
            ),
            (
                "https://github.com/owner/repo/issues/456",
                "/repos/owner/repo/issues/456",
            ),
            (
                "https://github.com/owner/repo/commit/abcdef",
                "/repos/owner/repo/commits/abcdef",
            ),
            (
                "https://github.com/owner/repo/releases",
                "/repos/owner/repo/releases",
            ),
            (
                "https://github.com/owner/repo/releases/tag/v1.2.3",
                "/repos/owner/repo/releases/tags/v1.2.3",
            ),
            (
                "https://github.com/owner/repo/releases/tag/release%2F2026",
                "/repos/owner/repo/releases/tags/release%2F2026",
            ),
            (
                "https://github.com/owner/repo/releases/latest",
                "/repos/owner/repo/releases/latest",
            ),
            (
                "https://github.com/owner/repo/compare/v1.0.0...v1.1.0",
                "/repos/owner/repo/compare/v1.0.0...v1.1.0",
            ),
        ] {
            assert_eq!(
                github_api_path_for(&url(input), &options.github_api_base_url),
                Some(expected.into()),
                "{input}"
            );
        }
    }

    #[test]
    fn test_github_browser_urls_reject_unhandled_paths_and_bad_numbers() {
        let options = VerifyOptions::default();
        for input in [
            "https://github.com",
            "https://github.com/owner/repo/pull/not-a-number",
            "https://github.com/owner/repo/issues/0",
            "https://github.com/owner/repo/pull/123/files",
            "https://github.com/owner/repo/blob/main/README.md",
        ] {
            assert_eq!(
                github_api_path_for(&url(input), &options.github_api_base_url),
                None,
                "{input} must not be classified as API-verifiable"
            );
        }

        assert_eq!(
            github_api_path_for(
                &url("https://github.com/owner/repo/commits/abcdef"),
                &options.github_api_base_url
            ),
            Some("/repos/owner/repo/commits/abcdef".into())
        );
        assert_eq!(
            github_api_path_for(
                &url("https://github.com/owner/repo/releases/tag/release/2026/04"),
                &options.github_api_base_url
            ),
            Some("/repos/owner/repo/releases/tags/release/2026/04".into())
        );
    }

    #[test]
    fn test_configured_github_api_base_path_mapping() {
        let api_base_url = Url::parse("https://github.example.test/api/v3/").unwrap();
        assert_eq!(
            github_api_path_for(
                &url("https://github.example.test/api/v3/repos/owner/repo/pulls/1"),
                &api_base_url
            ),
            Some("/repos/owner/repo/pulls/1".into())
        );
        assert_eq!(
            github_api_path_for(&url("https://github.example.test/api/v3"), &api_base_url),
            Some("/".into())
        );
        assert_eq!(
            github_api_path_for(
                &url("https://github.example.test/other/repos/owner/repo/pulls/1"),
                &api_base_url
            ),
            None
        );
    }

    #[test]
    fn test_github_status_reasons_preserve_actionable_context() {
        assert!(github_api_status_reason(StatusCode::UNAUTHORIZED).contains("rejected"));
        assert!(github_api_status_reason(StatusCode::FORBIDDEN).contains("permissions"));
        assert!(github_api_status_reason(StatusCode::NOT_FOUND).contains("lacks access"));
        assert_eq!(github_api_status_reason(StatusCode::BAD_GATEWAY), "502");

        assert!(
            github_browser_status_reason(StatusCode::FORBIDDEN, true)
                .contains("checked without authentication")
        );
        assert!(
            github_browser_status_reason(StatusCode::NOT_FOUND, false).contains("GITHUB_TOKEN")
        );
        assert_eq!(
            github_browser_status_reason(StatusCode::BAD_GATEWAY, true),
            "502"
        );
    }

    #[tokio::test]
    async fn test_verify_github_browser_link_uses_api_with_auth() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/pulls/123"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let text = "See https://github.com/owner/repo/pull/123";
        let broken = verify_with_options(&[text], &options_with_github_api(&server)).await;
        assert!(broken.is_empty());
    }

    #[tokio::test]
    async fn test_verify_github_api_link_uses_auth() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/issues/123"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let text = format!("See {}/repos/owner/repo/issues/123", server.uri());
        let broken = verify_with_options(&[&text], &options_with_github_api(&server)).await;
        assert!(broken.is_empty());
    }

    #[test]
    fn test_verify_github_link_without_token_reports_access_hint() {
        assert!(
            github_browser_status_reason(StatusCode::NOT_FOUND, false).contains("GITHUB_TOKEN")
        );
        assert!(
            github_browser_status_reason(StatusCode::FORBIDDEN, false).contains("GITHUB_TOKEN")
        );
    }

    #[tokio::test]
    async fn test_non_github_link_never_receives_auth() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let text = format!("See {}/page", server.uri());
        let options = VerifyOptions::new(
            Some("test-token"),
            Url::parse(DEFAULT_GITHUB_API_BASE_URL).unwrap(),
        );
        let broken = verify_with_options(&[&text], &options).await;
        assert!(broken.is_empty());

        let requests = server.received_requests().await.unwrap();
        assert!(!requests.is_empty());
        assert!(
            requests
                .iter()
                .all(|request| !request.headers.contains_key("authorization"))
        );
    }

    #[tokio::test]
    async fn test_authenticated_github_api_does_not_follow_cross_host_redirect() {
        let github_api = MockServer::start().await;
        let redirected = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/pulls/123"))
            .and(header("Authorization", "Bearer test-token"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}/capture", redirected.uri())),
            )
            .expect(1)
            .mount(&github_api)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&redirected)
            .await;

        let text = "See https://github.com/owner/repo/pull/123";
        let broken = verify_with_options(&[text], &options_with_github_api(&github_api)).await;
        assert_eq!(broken.len(), 1);
        assert!(broken[0].1.contains("302"));

        let redirected_requests = redirected.received_requests().await.unwrap();
        assert!(redirected_requests.is_empty());
    }
}
