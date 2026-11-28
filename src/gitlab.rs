//! Minimal GitLab REST API (v4) client for GitLab.com and self-managed instances.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::forge::{ChangeRequest, Issue, Release};

pub struct GitLabClient {
    client: reqwest::Client,
    token: String,
    project: String,
    api_base: String,
}

#[derive(Debug, Deserialize)]
struct GlRelease {
    tag_name: String,
    name: Option<String>,
    description: Option<String>,
}

impl From<GlRelease> for Release {
    fn from(r: GlRelease) -> Self {
        Self {
            id: None,
            tag_name: r.tag_name,
            name: r.name,
            body: r.description,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GlUser {
    username: String,
}

#[derive(Debug, Deserialize)]
struct GlMergeRequest {
    iid: u64,
    title: String,
    description: Option<String>,
    author: GlUser,
    #[serde(default)]
    labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GlIssue {
    iid: u64,
    title: String,
    description: Option<String>,
    state: String,
    author: GlUser,
    #[serde(default)]
    labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GlIssueRef {
    iid: u64,
    references: Option<GlReferences>,
}

#[derive(Debug, Deserialize)]
struct GlReferences {
    /// Fully qualified reference, e.g. `group/project#12`.
    full: String,
}

impl GlIssueRef {
    /// `#12` for an issue in `project`, otherwise the full `group/other#12`
    /// so the model does not attribute it to the wrong project.
    fn display(&self, project: &str) -> String {
        match &self.references {
            Some(r)
                if !r
                    .full
                    .eq_ignore_ascii_case(&format!("{project}#{}", self.iid)) =>
            {
                r.full.clone()
            }
            _ => format!("#{}", self.iid),
        }
    }
}

#[derive(Debug, Deserialize)]
struct GlDiff {
    old_path: String,
    new_path: String,
    diff: String,
    #[serde(default)]
    new_file: bool,
    #[serde(default)]
    deleted_file: bool,
    /// GitLab omits the diff text of large or collapsed files.
    #[serde(default)]
    collapsed: bool,
    #[serde(default)]
    too_large: bool,
}

#[derive(Debug, Deserialize)]
struct GlCommitMergeRequest {
    iid: u64,
    state: String,
}

#[derive(Debug, Serialize)]
struct ReleasePayload<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    tag_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
}

const DIFF_PAGE_SIZE: usize = 100;
/// Stop paging merge request diffs once this much text is collected; the
/// result is truncated further before it reaches the model.
const DIFF_COLLECT_LIMIT: usize = 64 * 1024;

/// `PRIVATE-TOKEN` is a custom header, so reqwest would forward it on a
/// cross-origin redirect. Follow redirects only within the original origin;
/// any other redirect is returned as-is and reported as an API error.
fn same_origin_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let same_origin = attempt
            .previous()
            .first()
            .is_some_and(|first| first.origin() == attempt.url().origin());
        if attempt.previous().len() > 5 {
            attempt.error("too many redirects")
        } else if same_origin {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

impl GitLabClient {
    /// `project` is the full project path (`group/subgroup/project`);
    /// `api_base` is the v4 API root, e.g. `https://gitlab.com/api/v4`.
    pub fn new(token: String, project: &str, api_base: &str) -> Result<Self> {
        let parsed = reqwest::Url::parse(api_base)
            .map_err(|e| Error::GitLab(format!("invalid GitLab API URL: {e}")))?;
        let host = parsed.host_str().unwrap_or_default();
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        // GITLAB_TOKEN must not travel in cleartext.
        if parsed.scheme() != "https" && !loopback {
            return Err(Error::GitLab(format!(
                "refusing to send GITLAB_TOKEN over {}; use an https:// GitLab URL",
                parsed.scheme()
            )));
        }
        let client = reqwest::Client::builder()
            .user_agent("communique/0.1")
            .redirect(same_origin_redirects())
            .build()?;
        Ok(Self {
            client,
            token,
            project: project.to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
        })
    }

    /// Build `<api>/projects/<url-encoded project>/<segments...>`. Each segment
    /// is percent-encoded, so `/` in project paths and tag names becomes `%2F`.
    fn url(&self, segments: &[&str]) -> Result<reqwest::Url> {
        let mut url = reqwest::Url::parse(&format!("{}/projects", self.api_base))
            .map_err(|e| Error::GitLab(format!("invalid GitLab API URL: {e}")))?;
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| Error::GitLab("invalid GitLab API URL".into()))?;
            path.push(&self.project);
            path.extend(segments);
        }
        Ok(url)
    }

    async fn send(
        &self,
        method: reqwest::Method,
        url: reqwest::Url,
        body: Option<&ReleasePayload<'_>>,
    ) -> Result<reqwest::Response> {
        crate::retry::retry_request("GitLab API", || {
            let mut req = self
                .client
                .request(method.clone(), url.clone())
                .header("PRIVATE-TOKEN", &self.token);
            if let Some(body) = body {
                req = req.json(body);
            }
            req.send()
        })
        .await
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: reqwest::Url,
        what: &str,
    ) -> Result<T> {
        let resp = self.send(reqwest::Method::GET, url, None).await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitLab(format!("GET {what}: {status} {body}")));
        }
        Ok(resp.json().await?)
    }

    /// Resolve a tag to the commit it points at (annotated tags are peeled).
    pub async fn tag_commit(&self, tag: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct Tag {
            commit: Commit,
        }
        #[derive(Deserialize)]
        struct Commit {
            id: String,
        }
        let url = self.url(&["repository", "tags", tag])?;
        let resp = self.send(reqwest::Method::GET, url, None).await?;
        if !resp.status().is_success() {
            return Err(Error::GitLab(format!(
                "Cannot resolve remote tag {tag}: {}",
                resp.status()
            )));
        }
        Ok(resp.json::<Tag>().await?.commit.id)
    }

    pub async fn get_release_by_tag(&self, tag: &str) -> Result<Option<Release>> {
        let url = self.url(&["releases", tag])?;
        let resp = self.send(reqwest::Method::GET, url, None).await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitLab(format!("GET release {tag}: {status} {body}")));
        }
        Ok(Some(resp.json::<GlRelease>().await?.into()))
    }

    pub async fn list_recent_releases(&self, count: u8) -> Result<Vec<Release>> {
        let mut url = self.url(&["releases"])?;
        url.query_pairs_mut()
            .append_pair("per_page", &count.to_string())
            .append_pair("order_by", "released_at")
            .append_pair("sort", "desc");
        let releases: Vec<GlRelease> = self.get_json(url, "releases").await?;
        Ok(releases.into_iter().map(Into::into).collect())
    }

    pub async fn update_release(
        &self,
        tag: &str,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<()> {
        let url = self.url(&["releases", tag])?;
        let payload = ReleasePayload {
            tag_name: None,
            name: title,
            description: body,
        };
        let resp = self.send(reqwest::Method::PUT, url, Some(&payload)).await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitLab(format!("PUT release {tag}: {status} {body}")));
        }
        Ok(())
    }

    /// Create a release for a tag that already exists in the project.
    pub async fn create_release(&self, tag: &str, title: Option<&str>, body: &str) -> Result<()> {
        let url = self.url(&["releases"])?;
        let payload = ReleasePayload {
            tag_name: Some(tag),
            name: title,
            description: Some(body),
        };
        let resp = self
            .send(reqwest::Method::POST, url, Some(&payload))
            .await?;
        if resp.status() == reqwest::StatusCode::CONFLICT {
            // Someone else created the release first (for example a retried
            // request that already succeeded). Accept it only if it matches.
            let existing = self.get_release_by_tag(tag).await?;
            let matches = existing.is_some_and(|r| {
                r.body.as_deref() == Some(body) && (title.is_none() || r.name.as_deref() == title)
            });
            if matches {
                return Ok(());
            }
            return Err(Error::GitLab(format!(
                "a different release for {tag} already exists; rerun to update it"
            )));
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitLab(format!(
                "POST release {tag}: {status} {body}"
            )));
        }
        Ok(())
    }

    pub async fn get_merge_request(&self, iid: u64) -> Result<ChangeRequest> {
        let iid_s = iid.to_string();
        let mr: GlMergeRequest = self
            .get_json(
                self.url(&["merge_requests", &iid_s])?,
                &format!("merge request !{iid}"),
            )
            .await?;
        // Linked issues are supplementary; a failure here should not hide the MR.
        let closes = match self
            .get_json::<Vec<GlIssueRef>>(
                self.url(&["merge_requests", &iid_s, "closes_issues"])?,
                &format!("issues closed by !{iid}"),
            )
            .await
        {
            Ok(issues) => issues.iter().map(|i| i.display(&self.project)).collect(),
            Err(e) => {
                log::info!("failed to fetch issues closed by !{iid}: {e}");
                vec![]
            }
        };
        Ok(ChangeRequest {
            number: mr.iid,
            title: mr.title,
            body: mr.description,
            author: mr.author.username,
            labels: mr.labels,
            closes,
        })
    }

    /// Return the merge request's changes as a unified diff.
    pub async fn get_merge_request_diff(&self, iid: u64) -> Result<String> {
        let iid_s = iid.to_string();
        let mut out = String::new();
        for page in 1.. {
            let mut url = self.url(&["merge_requests", &iid_s, "diffs"])?;
            url.query_pairs_mut()
                .append_pair("page", &page.to_string())
                .append_pair("per_page", &DIFF_PAGE_SIZE.to_string());
            let diffs: Vec<GlDiff> = self
                .get_json(url, &format!("merge request !{iid} diffs"))
                .await?;
            let count = diffs.len();
            for d in diffs {
                let old = if d.new_file {
                    "/dev/null".to_string()
                } else {
                    format!("a/{}", d.old_path)
                };
                let new = if d.deleted_file {
                    "/dev/null".to_string()
                } else {
                    format!("b/{}", d.new_path)
                };
                let text = if d.diff.is_empty() && (d.too_large || d.collapsed) {
                    "(diff omitted by GitLab: file too large)\n"
                } else {
                    d.diff.as_str()
                };
                out.push_str(&format!(
                    "diff --git a/{} b/{}\n--- {old}\n+++ {new}\n{text}",
                    d.old_path, d.new_path
                ));
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            if count < DIFF_PAGE_SIZE || out.len() > DIFF_COLLECT_LIMIT {
                break;
            }
        }
        Ok(out)
    }

    pub async fn get_issue(&self, iid: u64) -> Result<Issue> {
        let issue: GlIssue = self
            .get_json(
                self.url(&["issues", &iid.to_string()])?,
                &format!("issue #{iid}"),
            )
            .await?;
        Ok(Issue {
            number: issue.iid,
            title: issue.title,
            body: issue.description,
            state: issue.state,
            author: issue.author.username,
            labels: issue.labels,
        })
    }

    /// Merged merge requests that contain `sha`. Used to find merge requests
    /// for squashed or fast-forwarded commits whose messages don't mention them.
    pub async fn merged_requests_for_commit(&self, sha: &str) -> Result<Vec<u64>> {
        let mrs: Vec<GlCommitMergeRequest> = self
            .get_json(
                self.url(&["repository", "commits", sha, "merge_requests"])?,
                &format!("merge requests for {sha}"),
            )
            .await?;
        Ok(mrs
            .into_iter()
            .filter(|mr| mr.state == "merged")
            .map(|mr| mr.iid)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PROJECT: &str = "/api/v4/projects/group%2Fsub%2Fproj";

    async fn setup() -> (MockServer, GitLabClient) {
        let server = MockServer::start().await;
        let client = GitLabClient::new(
            "glpat-test".into(),
            "group/sub/proj",
            &format!("{}/api/v4", server.uri()),
        )
        .unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn release_lookup_encodes_project_and_tag() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/releases/cli%2Fv1.0.0")))
            .and(header("PRIVATE-TOKEN", "glpat-test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tag_name": "cli/v1.0.0",
                "name": "cli v1.0.0",
                "description": "notes"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let release = client
            .get_release_by_tag("cli/v1.0.0")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(release.tag_name, "cli/v1.0.0");
        assert_eq!(release.body.as_deref(), Some("notes"));
        assert_eq!(release.id, None);
    }

    #[tokio::test]
    async fn token_is_not_sent_across_origins_on_redirect() {
        let (server, client) = setup().await;
        let other = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/releases/v1.0.0")))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}/steal", other.uri())),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&other)
            .await;
        assert!(client.get_release_by_tag("v1.0.0").await.is_err());
        other.verify().await;
    }

    #[tokio::test]
    async fn missing_release_is_none() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/releases/v1.0.0")))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert!(client.get_release_by_tag("v1.0.0").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn creates_and_updates_releases() {
        let (server, client) = setup().await;
        Mock::given(method("POST"))
            .and(path(format!("{PROJECT}/releases")))
            .and(body_json(json!({
                "tag_name": "v1.0.0",
                "name": "v1.0.0: title",
                "description": "body"
            })))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path(format!("{PROJECT}/releases/v1.0.0")))
            .and(body_json(json!({"description": "new body"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        client
            .create_release("v1.0.0", Some("v1.0.0: title"), "body")
            .await
            .unwrap();
        client
            .update_release("v1.0.0", None, Some("new body"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn merge_request_includes_closed_issues() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/merge_requests/12")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "iid": 12,
                "title": "Add widgets",
                "description": "Adds widgets",
                "author": {"username": "ada"},
                "labels": ["feature"]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/merge_requests/12/closes_issues")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"iid": 3, "references": {"full": "group/sub/proj#3"}},
                {"iid": 4, "references": {"full": "group/other#4"}},
                {"iid": 5}
            ])))
            .mount(&server)
            .await;
        let mr = client.get_merge_request(12).await.unwrap();
        assert_eq!(mr.number, 12);
        assert_eq!(mr.author, "ada");
        assert_eq!(mr.labels, vec!["feature"]);
        assert_eq!(mr.closes, vec!["#3", "group/other#4", "#5"]);
    }

    #[tokio::test]
    async fn merge_request_survives_closed_issue_failure() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/merge_requests/12")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "iid": 12,
                "title": "Add widgets",
                "description": null,
                "author": {"username": "ada"}
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/merge_requests/12/closes_issues")))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let mr = client.get_merge_request(12).await.unwrap();
        assert!(mr.closes.is_empty());
        assert!(mr.labels.is_empty());
    }

    #[tokio::test]
    async fn merge_request_diff_is_rendered_as_unified_diff() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/merge_requests/12/diffs")))
            .and(query_param("page", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"old_path": "a.rs", "new_path": "a.rs", "diff": "@@ -1 +1 @@\n-a\n+b\n"},
                {"old_path": "new.rs", "new_path": "new.rs", "diff": "@@ -0,0 +1 @@\n+c", "new_file": true},
                {"old_path": "big.json", "new_path": "big.json", "diff": "", "too_large": true}
            ])))
            .expect(1)
            .mount(&server)
            .await;
        let diff = client.get_merge_request_diff(12).await.unwrap();
        assert!(diff.contains("diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@"));
        assert!(diff.contains("--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1 @@\n+c\n"));
        assert!(diff.contains("+++ b/big.json\n(diff omitted by GitLab: file too large)\n"));
    }

    #[test]
    fn token_requires_https_except_on_loopback() {
        let err = GitLabClient::new("t".into(), "g/p", "http://gitlab.example.com/api/v4")
            .err()
            .unwrap();
        assert!(err.to_string().contains("https://"), "{err}");
        for ok in [
            "https://gitlab.example.com/api/v4",
            "http://127.0.0.1:8080/api/v4",
            "http://localhost/api/v4",
            "http://[::1]:8080/api/v4",
        ] {
            assert!(GitLabClient::new("t".into(), "g/p", ok).is_ok(), "{ok}");
        }
    }

    #[tokio::test]
    async fn create_conflict_accepts_only_an_identical_release() {
        let (server, client) = setup().await;
        Mock::given(method("POST"))
            .and(path(format!("{PROJECT}/releases")))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/releases/v1.0.0")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "tag_name": "v1.0.0",
                "name": "v1.0.0: title",
                "description": "body"
            })))
            .mount(&server)
            .await;
        client
            .create_release("v1.0.0", Some("v1.0.0: title"), "body")
            .await
            .unwrap();
        let err = client
            .create_release("v1.0.0", Some("v1.0.0: title"), "other body")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
    }

    #[tokio::test]
    async fn issue_and_tag_lookup() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/issues/3")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "iid": 3,
                "title": "Bug",
                "description": "Broken",
                "state": "closed",
                "author": {"username": "reporter"},
                "labels": ["bug"]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{PROJECT}/repository/tags/v1.0.0")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"name": "v1.0.0", "commit": {"id": "abc"}})),
            )
            .mount(&server)
            .await;
        let issue = client.get_issue(3).await.unwrap();
        assert_eq!((issue.number, issue.state.as_str()), (3, "closed"));
        assert_eq!(client.tag_commit("v1.0.0").await.unwrap(), "abc");
    }

    #[tokio::test]
    async fn finds_merged_requests_for_a_commit() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path(format!(
                "{PROJECT}/repository/commits/abc123/merge_requests"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"iid": 5, "state": "merged"},
                {"iid": 6, "state": "closed"}
            ])))
            .mount(&server)
            .await;
        assert_eq!(
            client.merged_requests_for_commit("abc123").await.unwrap(),
            vec![5]
        );
    }
}
