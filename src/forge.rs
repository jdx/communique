//! The code-hosting service ("forge") that holds the repository: GitHub or GitLab.
//!
//! [`Repo`] identifies the project and builds web links for prompts and reports.
//! [`ForgeClient`] wraps the API client for whichever forge the repo lives on,
//! exposing the forge-neutral operations the rest of communique needs.

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::github::GitHubClient;
use crate::gitlab::GitLabClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, strum::EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ForgeKind {
    #[default]
    GitHub,
    GitLab,
}

impl ForgeKind {
    pub fn name(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "GitHub",
            ForgeKind::GitLab => "GitLab",
        }
    }

    pub fn token_var(self) -> &'static str {
        match self {
            ForgeKind::GitHub => "GITHUB_TOKEN",
            ForgeKind::GitLab => "GITLAB_TOKEN",
        }
    }

    /// How the forge writes a reference to a pull/merge request: `#12` or `!12`.
    pub fn change_ref(self, number: u64) -> String {
        match self {
            ForgeKind::GitHub => format!("#{number}"),
            ForgeKind::GitLab => format!("!{number}"),
        }
    }

    pub fn is_default(&self) -> bool {
        *self == ForgeKind::GitHub
    }
}

/// A repository on a specific forge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub kind: ForgeKind,
    /// `owner/repo` on GitHub; the full project path (`group/subgroup/project`) on GitLab.
    pub path: String,
    /// Web root of the forge without a trailing slash, e.g. `https://gitlab.example.com`.
    pub web_base: String,
}

impl Repo {
    pub fn github(path: &str) -> Self {
        Self {
            kind: ForgeKind::GitHub,
            path: path.to_string(),
            web_base: "https://github.com".into(),
        }
    }

    pub fn gitlab(path: &str, web_base: &str) -> Self {
        Self {
            kind: ForgeKind::GitLab,
            path: path.to_string(),
            web_base: web_base.trim_end_matches('/').to_string(),
        }
    }

    pub fn web_url(&self) -> String {
        format!("{}/{}", self.web_base, self.path)
    }

    pub fn commit_url(&self, sha: &str) -> String {
        match self.kind {
            ForgeKind::GitHub => format!("{}/commit/{sha}", self.web_url()),
            ForgeKind::GitLab => format!("{}/-/commit/{sha}", self.web_url()),
        }
    }

    pub fn release_url(&self, tag: &str) -> String {
        match self.kind {
            ForgeKind::GitHub => format!("{}/releases/tag/{tag}", self.web_url()),
            ForgeKind::GitLab => format!("{}/-/releases/{tag}", self.web_url()),
        }
    }

    /// The forge's web root when it differs from the forge's public default,
    /// so it can be stored in drafts only when needed.
    pub fn custom_web_base(&self) -> Option<&str> {
        match self.kind {
            ForgeKind::GitHub => None,
            ForgeKind::GitLab => {
                (self.web_base != "https://gitlab.com").then_some(self.web_base.as_str())
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        let segments: Vec<_> = self.path.split('/').collect();
        let valid_segment = |s: &&str| {
            !s.is_empty()
                && *s != "."
                && *s != ".."
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        };
        let ok = match self.kind {
            ForgeKind::GitHub => segments.len() == 2,
            ForgeKind::GitLab => segments.len() >= 2,
        } && segments.iter().all(valid_segment);
        if ok {
            Ok(())
        } else {
            let expected = match self.kind {
                ForgeKind::GitHub => "owner/repo",
                ForgeKind::GitLab => "group/project or group/subgroup/project",
            };
            Err(Error::Config(format!(
                "invalid {} repository '{}' (expected {expected})",
                self.kind.name(),
                self.path
            )))
        }
    }
}

/// Inputs for [`resolve_repo`], gathered from the CLI, `communique.toml`,
/// the git remote, and the environment.
#[derive(Debug, Default)]
pub struct RepoSettings {
    pub repo: Option<String>,
    pub forge: Option<ForgeKind>,
    pub forge_url: Option<String>,
    pub remote_url: Option<String>,
}

/// Decide which forge and project the release belongs to.
///
/// Precedence: an explicit `forge`/`forge_url`, then GitLab CI's predefined
/// variables, then the `origin` remote's host. A `repo` override without a
/// `forge` keeps the historical meaning of a GitHub `owner/repo`.
pub fn resolve_repo(settings: RepoSettings, env: impl Fn(&str) -> Option<String>) -> Result<Repo> {
    let env = |name: &str| env(name).filter(|v| !v.is_empty());
    let forge_url = settings
        .forge_url
        .filter(|u| !u.is_empty())
        .map(|u| u.trim_end_matches('/').to_string());
    if let Some(url) = &forge_url
        && !(url.starts_with("https://") || url.starts_with("http://"))
    {
        return Err(Error::Config(format!(
            "forge_url must start with https:// or http://: {url}"
        )));
    }
    let remote = settings.remote_url.as_deref().and_then(parse_remote);
    let gitlab_ci = env("GITLAB_CI").is_some_and(|v| v == "true");
    let ci_project = gitlab_ci
        .then(|| env("CI_SERVER_URL").zip(env("CI_PROJECT_PATH")))
        .flatten();

    let kind = match settings.forge {
        Some(kind) => kind,
        None if forge_url.is_some() => ForgeKind::GitLab,
        None if settings.repo.is_some() => ForgeKind::GitHub,
        None if ci_project.is_some() => ForgeKind::GitLab,
        None => match remote.as_ref().map(|r| r.host.as_str()) {
            Some("github.com") => ForgeKind::GitHub,
            Some("gitlab.com") => ForgeKind::GitLab,
            Some(host) => {
                return Err(Error::Git(format!(
                    "cannot tell which forge hosts {host}; set `forge = \"gitlab\"` (and `forge_url` for a self-managed instance) under [defaults] in communique.toml"
                )));
            }
            None => {
                return Err(match &settings.remote_url {
                    Some(url) => {
                        Error::Git(format!("cannot parse repository from remote URL: {url}"))
                    }
                    None => Error::Git("no origin remote found".into()),
                });
            }
        },
    };

    let repo = match kind {
        ForgeKind::GitHub => {
            if forge_url.is_some() {
                return Err(Error::Config(
                    "forge_url is only supported with forge = \"gitlab\"".into(),
                ));
            }
            let path = match settings.repo {
                Some(path) => path,
                None => match remote {
                    Some(r) if r.host == "github.com" => r.path,
                    _ => {
                        return Err(Error::Git(format!(
                            "cannot parse GitHub repo from remote URL: {}",
                            settings.remote_url.as_deref().unwrap_or("(none)")
                        )));
                    }
                },
            };
            Repo::github(&path)
        }
        ForgeKind::GitLab => {
            let ci = ci_project.as_ref();
            let web_base = forge_url
                .or_else(|| ci.map(|(url, _)| url.clone()))
                .or_else(|| remote.as_ref().map(|r| r.web_base.clone()))
                .ok_or_else(|| {
                    Error::Config("cannot determine the GitLab URL; set forge_url".into())
                })?;
            let path = settings
                .repo
                .or_else(|| ci.map(|(_, path)| path.clone()))
                .or_else(|| remote.map(|r| r.path))
                .ok_or_else(|| {
                    Error::Config("cannot determine the GitLab project path; set repo".into())
                })?;
            Repo::gitlab(&path, &web_base)
        }
    };
    repo.validate()?;
    Ok(repo)
}

#[derive(Debug, PartialEq)]
struct Remote {
    host: String,
    web_base: String,
    path: String,
}

/// Parse `git@host:path.git`, `ssh://[user@]host[:port]/path.git`, or
/// `https://[credentials@]host[:port]/path.git`.
fn parse_remote(url: &str) -> Option<Remote> {
    let (host_port, path, web_scheme_keeps_port) = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        let (authority, path) = rest.split_once('/')?;
        (authority, path, true)
    } else if let Some(rest) = url.strip_prefix("ssh://") {
        let (authority, path) = rest.split_once('/')?;
        (authority, path, false)
    } else {
        // scp-like syntax: [user@]host:path
        let (authority, path) = url.split_once(':')?;
        if authority.contains('/') {
            return None;
        }
        (authority, path, false)
    };
    // Drop credentials such as `gitlab-ci-token:<token>@`.
    let host_port = host_port.rsplit_once('@').map_or(host_port, |(_, h)| h);
    let host = host_port.split(':').next()?.to_ascii_lowercase();
    let path = path.trim_matches('/').trim_end_matches(".git").to_string();
    if host.is_empty() || !path.contains('/') {
        return None;
    }
    let scheme = if url.starts_with("http://") {
        "http"
    } else {
        "https"
    };
    let authority = if web_scheme_keeps_port {
        host_port.to_ascii_lowercase()
    } else {
        host.clone()
    };
    Some(Remote {
        web_base: format!("{scheme}://{authority}"),
        host,
        path,
    })
}

#[derive(Debug, Clone)]
pub struct Release {
    /// GitHub's numeric release id. GitLab addresses releases by tag.
    pub id: Option<u64>,
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug)]
pub struct ChangeRequest {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub author: String,
    pub labels: Vec<String>,
    /// Issues the merge request closes (GitLab only).
    pub closes: Vec<u64>,
}

#[derive(Debug)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    pub author: String,
    pub labels: Vec<String>,
}

pub enum ForgeClient {
    GitHub(GitHubClient),
    GitLab(GitLabClient),
}

impl ForgeClient {
    pub fn new(repo: &Repo, token: String) -> Result<Self> {
        Ok(match repo.kind {
            ForgeKind::GitHub => ForgeClient::GitHub(GitHubClient::new(token, &repo.path)?),
            ForgeKind::GitLab => ForgeClient::GitLab(GitLabClient::new(
                token,
                &repo.path,
                &format!("{}/api/v4", repo.web_base),
            )?),
        })
    }

    pub fn kind(&self) -> ForgeKind {
        match self {
            ForgeClient::GitHub(_) => ForgeKind::GitHub,
            ForgeClient::GitLab(_) => ForgeKind::GitLab,
        }
    }

    pub async fn tag_commit(&self, tag: &str) -> Result<String> {
        match self {
            ForgeClient::GitHub(c) => c.tag_commit(tag).await,
            ForgeClient::GitLab(c) => c.tag_commit(tag).await,
        }
    }

    pub async fn get_release_by_tag(&self, tag: &str) -> Result<Option<Release>> {
        match self {
            ForgeClient::GitHub(c) => Ok(c.get_release_by_tag(tag).await?.map(Into::into)),
            ForgeClient::GitLab(c) => c.get_release_by_tag(tag).await,
        }
    }

    pub async fn list_recent_releases(&self, count: u8) -> Result<Vec<Release>> {
        match self {
            ForgeClient::GitHub(c) => Ok(c
                .list_recent_releases(count)
                .await?
                .into_iter()
                .map(Into::into)
                .collect()),
            ForgeClient::GitLab(c) => c.list_recent_releases(count).await,
        }
    }

    pub async fn update_release(
        &self,
        release: &Release,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<()> {
        match self {
            ForgeClient::GitHub(c) => {
                let id = release.id.ok_or_else(|| {
                    Error::GitHub(format!("release {} has no id", release.tag_name))
                })?;
                c.update_release(id, &release.tag_name, title, body).await
            }
            ForgeClient::GitLab(c) => c.update_release(&release.tag_name, title, body).await,
        }
    }

    /// Create a release for an existing tag. Only GitLab releases are created;
    /// GitHub releases are expected to exist already (usually as drafts).
    pub async fn create_release(&self, tag: &str, title: Option<&str>, body: &str) -> Result<()> {
        match self {
            ForgeClient::GitHub(_) => {
                Err(Error::GitHub(format!("No GitHub release found for {tag}")))
            }
            ForgeClient::GitLab(c) => c.create_release(tag, title, body).await,
        }
    }

    pub async fn get_change_request(&self, number: u64) -> Result<ChangeRequest> {
        match self {
            ForgeClient::GitHub(c) => {
                let pr = c.get_pr(number).await?;
                Ok(ChangeRequest {
                    number: pr.number,
                    title: pr.title,
                    body: pr.body,
                    author: pr.user.login,
                    labels: pr.labels.into_iter().map(|l| l.name).collect(),
                    closes: vec![],
                })
            }
            ForgeClient::GitLab(c) => c.get_merge_request(number).await,
        }
    }

    pub async fn get_change_request_diff(&self, number: u64) -> Result<String> {
        let diff = match self {
            ForgeClient::GitHub(c) => c.get_pr_diff(number).await?,
            ForgeClient::GitLab(c) => c.get_merge_request_diff(number).await?,
        };
        Ok(truncate_diff(diff))
    }

    pub async fn get_issue(&self, number: u64) -> Result<Issue> {
        match self {
            ForgeClient::GitHub(c) => {
                let issue = c.get_issue(number).await?;
                Ok(Issue {
                    number: issue.number,
                    title: issue.title,
                    body: issue.body,
                    state: issue.state,
                    author: issue.user.login,
                    labels: issue.labels.into_iter().map(|l| l.name).collect(),
                })
            }
            ForgeClient::GitLab(c) => c.get_issue(number).await,
        }
    }
}

const MAX_DIFF_BYTES: usize = 50_000;

/// Truncate very large diffs to avoid blowing up the model's context.
fn truncate_diff(diff: String) -> String {
    if diff.len() <= MAX_DIFF_BYTES {
        return diff;
    }
    let mut end = MAX_DIFF_BYTES;
    while !diff.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...\n\n[diff truncated at 50KB]", &diff[..end])
}

impl From<crate::github::Release> for Release {
    fn from(r: crate::github::Release) -> Self {
        Self {
            id: Some(r.id),
            tag_name: r.tag_name,
            name: r.name,
            body: r.body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn settings(remote: &str) -> RepoSettings {
        RepoSettings {
            remote_url: Some(remote.into()),
            ..Default::default()
        }
    }

    #[test]
    fn parses_common_remote_forms() {
        for (url, host, base, path) in [
            (
                "git@github.com:jdx/communique.git",
                "github.com",
                "https://github.com",
                "jdx/communique",
            ),
            (
                "https://github.com/jdx/communique.git",
                "github.com",
                "https://github.com",
                "jdx/communique",
            ),
            (
                "ssh://git@gitlab.example.com:2222/group/sub/proj.git",
                "gitlab.example.com",
                "https://gitlab.example.com",
                "group/sub/proj",
            ),
            (
                "https://gitlab-ci-token:secret@gitlab.example.com:8443/group/proj.git",
                "gitlab.example.com",
                "https://gitlab.example.com:8443",
                "group/proj",
            ),
            (
                "http://gitlab.local/group/proj",
                "gitlab.local",
                "http://gitlab.local",
                "group/proj",
            ),
        ] {
            let remote = parse_remote(url).unwrap_or_else(|| panic!("{url}"));
            assert_eq!(remote.host, host, "{url}");
            assert_eq!(remote.web_base, base, "{url}");
            assert_eq!(remote.path, path, "{url}");
        }
        assert_eq!(parse_remote("/local/path/repo"), None);
    }

    #[test]
    fn detects_github_and_gitlab_com_from_remote() {
        let repo = resolve_repo(settings("git@github.com:jdx/communique.git"), no_env).unwrap();
        assert_eq!(repo, Repo::github("jdx/communique"));

        let repo = resolve_repo(settings("git@gitlab.com:group/sub/proj.git"), no_env).unwrap();
        assert_eq!(repo, Repo::gitlab("group/sub/proj", "https://gitlab.com"));
        assert_eq!(
            repo.commit_url("abc1234"),
            "https://gitlab.com/group/sub/proj/-/commit/abc1234"
        );
        assert_eq!(repo.custom_web_base(), None);
    }

    #[test]
    fn unknown_host_requires_explicit_forge() {
        let err = resolve_repo(settings("git@git.example.com:group/proj.git"), no_env).unwrap_err();
        assert!(err.to_string().contains("forge = \"gitlab\""), "{err}");

        let repo = resolve_repo(
            RepoSettings {
                forge: Some(ForgeKind::GitLab),
                ..settings("git@git.example.com:group/proj.git")
            },
            no_env,
        )
        .unwrap();
        assert_eq!(repo, Repo::gitlab("group/proj", "https://git.example.com"));
    }

    #[test]
    fn forge_url_selects_self_managed_gitlab() {
        let repo = resolve_repo(
            RepoSettings {
                forge_url: Some("https://gitlab.example.com/".into()),
                ..settings("git@mirror.example.com:group/proj.git")
            },
            no_env,
        )
        .unwrap();
        assert_eq!(
            repo,
            Repo::gitlab("group/proj", "https://gitlab.example.com")
        );
        assert_eq!(repo.custom_web_base(), Some("https://gitlab.example.com"));
    }

    #[test]
    fn gitlab_ci_variables_identify_the_project() {
        let env = |name: &str| match name {
            "GITLAB_CI" => Some("true".to_string()),
            "CI_SERVER_URL" => Some("https://gitlab.example.com".to_string()),
            "CI_PROJECT_PATH" => Some("group/sub/proj".to_string()),
            _ => None,
        };
        let repo = resolve_repo(
            settings("https://gitlab-ci-token:secret@gitlab.example.com/group/sub/proj.git"),
            env,
        )
        .unwrap();
        assert_eq!(
            repo,
            Repo::gitlab("group/sub/proj", "https://gitlab.example.com")
        );
    }

    #[test]
    fn repo_override_without_forge_stays_github() {
        let repo = resolve_repo(
            RepoSettings {
                repo: Some("owner/repo".into()),
                ..settings("git@gitlab.com:group/proj.git")
            },
            no_env,
        )
        .unwrap();
        assert_eq!(repo, Repo::github("owner/repo"));
    }

    #[test]
    fn forge_url_is_rejected_for_github() {
        let err = resolve_repo(
            RepoSettings {
                forge: Some(ForgeKind::GitHub),
                forge_url: Some("https://ghe.example.com".into()),
                ..settings("git@github.com:o/r.git")
            },
            no_env,
        )
        .unwrap_err();
        assert!(err.to_string().contains("forge_url"), "{err}");
    }

    #[test]
    fn validates_paths_per_forge() {
        assert!(Repo::github("a/b/c").validate().is_err());
        assert!(
            Repo::gitlab("a/b/c", "https://gitlab.com")
                .validate()
                .is_ok()
        );
        assert!(Repo::gitlab("a", "https://gitlab.com").validate().is_err());
        assert!(
            Repo::gitlab("a/../b", "https://gitlab.com")
                .validate()
                .is_err()
        );
        assert!(
            Repo::gitlab("a//b", "https://gitlab.com")
                .validate()
                .is_err()
        );
    }

    #[test]
    fn truncates_large_diffs_on_char_boundaries() {
        let diff = "é".repeat(MAX_DIFF_BYTES);
        let truncated = truncate_diff(diff);
        assert!(truncated.ends_with("[diff truncated at 50KB]"));
    }
}
