use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub struct GitHubClient {
    client: reqwest::Client,
    token: String,
    owner: String,
    repo: String,
    base_url: String,
}

#[derive(Debug, Deserialize)]
pub struct Release {
    pub id: u64,
    #[allow(dead_code)]
    pub tag_name: String,
    #[allow(dead_code)]
    pub name: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub user: User,
    pub labels: Vec<Label>,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct Label {
    pub name: String,
}

#[derive(Debug, Serialize)]
struct UpdateRelease {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<String>,
}

impl GitHubClient {
    pub fn new(token: String, owner_repo: &str) -> Result<Self> {
        Self::with_base_url(token, owner_repo, "https://api.github.com".into())
    }

    pub(crate) fn with_base_url(token: String, owner_repo: &str, base_url: String) -> Result<Self> {
        let (owner, repo) = owner_repo
            .split_once('/')
            .ok_or_else(|| Error::GitHub(format!("invalid owner/repo: {owner_repo}")))?;
        let client = reqwest::Client::builder()
            .user_agent("communique/0.1")
            .build()?;
        Ok(Self {
            client,
            token,
            owner: owner.to_string(),
            repo: repo.to_string(),
            base_url,
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/repos/{}/{}{path}", self.base_url, self.owner, self.repo)
    }

    pub async fn get_release_by_tag(&self, tag: &str) -> Result<Option<Release>> {
        let url = self.api_url(&format!("/releases/tags/{tag}"));
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitHub(format!("GET release {tag}: {status} {body}")));
        }
        Ok(Some(resp.json().await?))
    }

    pub async fn update_release(
        &self,
        release_id: u64,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<()> {
        let url = self.api_url(&format!("/releases/{release_id}"));
        let payload = UpdateRelease {
            name: title.map(String::from),
            body: body.map(String::from),
        };
        let resp = self
            .client
            .patch(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitHub(format!(
                "PATCH release {release_id}: {status} {body}"
            )));
        }
        Ok(())
    }

    pub async fn list_recent_releases(&self, count: u8) -> Result<Vec<Release>> {
        let url = self.api_url(&format!("/releases?per_page={count}"));
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitHub(format!("GET releases: {status} {body}")));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_pr(&self, number: u64) -> Result<PullRequest> {
        let url = self.api_url(&format!("/pulls/{number}"));
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitHub(format!("GET PR #{number}: {status} {body}")));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_pr_diff(&self, number: u64) -> Result<String> {
        let url = self.api_url(&format!("/pulls/{number}"));
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github.v3.diff")
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::GitHub(format!(
                "GET PR #{number} diff: {status} {body}"
            )));
        }
        let diff = resp.text().await?;
        // Truncate very large diffs to avoid blowing up context
        if diff.len() > 50_000 {
            Ok(format!(
                "{}...\n\n[diff truncated at 50KB]",
                &diff[..50_000]
            ))
        } else {
            Ok(diff)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn setup() -> (MockServer, GitHubClient) {
        let server = MockServer::start().await;
        let client =
            GitHubClient::with_base_url("test-token".into(), "owner/repo", server.uri()).unwrap();
        (server, client)
    }

    #[test]
    fn test_new_invalid_owner_repo() {
        let result = GitHubClient::new("token".into(), "invalid");
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("invalid owner/repo")
        );
    }

    #[tokio::test]
    async fn test_get_release_by_tag() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/releases/tags/v1.0.0"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": 123,
                "tag_name": "v1.0.0",
                "name": "Version 1.0.0",
                "body": "Release notes"
            })))
            .mount(&server)
            .await;

        let release = client.get_release_by_tag("v1.0.0").await.unwrap().unwrap();
        assert_eq!(release.id, 123);
        assert_eq!(release.body.as_deref(), Some("Release notes"));
    }

    #[tokio::test]
    async fn test_get_release_by_tag_not_found() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/releases/tags/v9.9.9"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let result = client.get_release_by_tag("v9.9.9").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_update_release() {
        let (server, client) = setup().await;
        Mock::given(method("PATCH"))
            .and(path("/repos/owner/repo/releases/123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 123})))
            .mount(&server)
            .await;

        client
            .update_release(123, Some("Title"), Some("Body"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_list_recent_releases() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([
                {"id": 1, "tag_name": "v2.0.0", "name": "v2", "body": "Notes 2"},
                {"id": 2, "tag_name": "v1.0.0", "name": "v1", "body": "Notes 1"},
            ])))
            .mount(&server)
            .await;

        let releases = client.list_recent_releases(3).await.unwrap();
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].tag_name, "v2.0.0");
    }

    #[tokio::test]
    async fn test_get_pr() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "number": 42,
                "title": "Add feature",
                "body": "Description",
                "user": {"login": "testuser"},
                "labels": [{"name": "enhancement"}]
            })))
            .mount(&server)
            .await;

        let pr = client.get_pr(42).await.unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.title, "Add feature");
        assert_eq!(pr.user.login, "testuser");
        assert_eq!(pr.labels[0].name, "enhancement");
    }

    #[tokio::test]
    async fn test_get_pr_diff() {
        let (server, client) = setup().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_string("diff --git a/file.rs"))
            .mount(&server)
            .await;

        let diff = client.get_pr_diff(42).await.unwrap();
        assert!(diff.contains("diff --git"));
    }

    #[tokio::test]
    async fn test_get_pr_diff_truncation() {
        let (server, client) = setup().await;
        let large_diff = "x".repeat(100_000);
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&large_diff))
            .mount(&server)
            .await;

        let diff = client.get_pr_diff(42).await.unwrap();
        assert!(diff.contains("[diff truncated at 50KB]"));
        assert!(diff.len() < 100_000);
    }
}
