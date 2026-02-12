use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub struct GitHubClient {
    client: reqwest::Client,
    token: String,
    owner: String,
    repo: String,
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
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!(
            "https://api.github.com/repos/{}/{}{path}",
            self.owner, self.repo
        )
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
