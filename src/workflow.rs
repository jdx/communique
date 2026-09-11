use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use miette::IntoDiagnostic;
use serde::{Deserialize, Serialize};

use crate::{github::GitHubClient, output::ParsedOutput};

#[derive(Debug, Default, Clone, usage_rs::Args)]
pub struct WorkflowOptions {
    /// Save editable release title, body, changelog, and review as JSON
    #[usage(long, effect = "write")]
    pub draft: Option<PathBuf>,
    /// Write a Markdown coverage report with sources and omission reasons
    #[usage(long, effect = "write")]
    pub review_report: Option<PathBuf>,
    /// Write an upgrade guide with affected users and before/after examples
    #[usage(long, effect = "write")]
    pub migration_guide: Option<PathBuf>,
    /// Generate notes for a package configured in communique.toml
    #[usage(long)]
    pub package: Option<String>,
    /// Limit changes to repository-relative paths (repeatable)
    #[usage(long = "path")]
    pub paths: Vec<String>,
    /// Only consider baseline tags matching this git glob
    #[usage(long)]
    pub tag_pattern: Option<String>,
    /// Baseline channel: stable excludes prereleases; all includes them
    #[usage(long, choices("stable", "all"))]
    pub channel: Option<Channel>,
    /// Changelog destination relative to the repository root
    #[usage(long)]
    pub changelog_path: Option<PathBuf>,
    /// Replace only the communique-marked section of a GitHub release
    #[usage(long)]
    pub preserve_sections: bool,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, strum::EnumString)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Channel {
    Stable,
    #[default]
    All,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    #[serde(default)]
    pub paths: Vec<String>,
    pub tag_pattern: Option<String>,
    pub channel: Option<Channel>,
    pub changelog_path: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    #[serde(default)]
    pub include_labels: Vec<String>,
    #[serde(default)]
    pub exclude_labels: Vec<String>,
    #[serde(default)]
    pub categories: BTreeMap<String, String>,
}

impl WorkflowOptions {
    pub fn resolve(&mut self, config: &crate::config::Config) -> miette::Result<()> {
        if let Some(name) = &self.package {
            let package = config
                .packages
                .get(name)
                .ok_or_else(|| miette::miette!("Unknown package '{name}' in communique.toml"))?;
            if self.paths.is_empty() {
                self.paths = package.paths.clone();
            }
            self.tag_pattern = self.tag_pattern.take().or(package.tag_pattern.clone());
            self.channel = self.channel.or(package.channel);
            self.changelog_path = self
                .changelog_path
                .take()
                .or(package.changelog_path.clone());
        }
        for path in self
            .paths
            .iter()
            .map(Path::new)
            .chain(self.changelog_path.iter().map(PathBuf::as_path))
        {
            if path.as_os_str().is_empty()
                || path.components().any(|c| {
                    !matches!(
                        c,
                        std::path::Component::Normal(_) | std::path::Component::CurDir
                    )
                })
            {
                return Err(miette::miette!(
                    "Paths must be relative to the repository and cannot contain '..': {}",
                    path.display()
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Review {
    pub coverage: Vec<Coverage>,
    pub migration_guide: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    pub commit: String,
    pub status: String,
    pub reason: String,
}

impl Review {
    pub fn from_submission(input: &serde_json::Value) -> Self {
        Self {
            coverage: serde_json::from_value(input["coverage"].clone()).unwrap_or_default(),
            migration_guide: input["migration_guide"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        }
    }

    /// Keep only known commits, fill gaps explicitly, and preserve enforced exclusions.
    pub fn reconcile(&mut self, log: &str, excluded: Vec<Coverage>) {
        let mut entries = Vec::new();
        for line in log.lines() {
            let sha = line.split_whitespace().next().unwrap_or_default();
            let matches: Vec<_> = self.coverage.iter().filter(|c| c.commit == sha).collect();
            let entry = if matches.len() == 1
                && ["included", "omitted", "uncertain"].contains(&matches[0].status.as_str())
                && !matches[0].reason.trim().is_empty()
            {
                matches[0].clone()
            } else {
                Coverage { commit: sha.into(), status: "uncertain".into(), reason: "The model did not provide a valid, unique assessment; review this change manually.".into() }
            };
            entries.push(entry);
        }
        entries.extend(excluded);
        self.coverage = entries;
    }

    pub fn markdown(&self, repo: &str, from: &str, to: &str) -> String {
        let mut result = format!(
            "# Release coverage\n\nRange: `{from}..{to}`\n\nAssessments are model-generated, except explicit label exclusions. Uncertain entries require manual review.\n"
        );
        for entry in &self.coverage {
            result.push_str(&format!(
                "\n- **{}** [{}](https://github.com/{repo}/commit/{}): {}\n",
                entry.status, entry.commit, entry.commit, entry.reason
            ));
        }
        result
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub schema_version: u32,
    pub repo: String,
    pub tag: String,
    pub previous_ref: String,
    pub target_commit: String,
    pub release_title: String,
    pub release_body: String,
    pub changelog: String,
    pub preserve_sections: bool,
    pub review: Review,
}

impl Draft {
    pub fn new(
        repo: &str,
        tag: &str,
        previous_ref: &str,
        target_commit: &str,
        notes: &ParsedOutput,
        preserve_sections: bool,
    ) -> Self {
        Self {
            schema_version: 1,
            repo: repo.into(),
            tag: tag.into(),
            previous_ref: previous_ref.into(),
            target_commit: target_commit.into(),
            release_title: notes.release_title.clone(),
            release_body: notes.release_body.clone(),
            changelog: notes.changelog.clone(),
            preserve_sections,
            review: notes.review.clone(),
        }
    }
    pub fn write(&self, path: &Path) -> miette::Result<()> {
        std::fs::write(
            path,
            format!(
                "{}\n",
                serde_json::to_string_pretty(self).into_diagnostic()?
            ),
        )
        .into_diagnostic()
    }
    fn validate(&self) -> miette::Result<()> {
        if self.schema_version != 1 {
            return Err(miette::miette!(
                "Unsupported draft schema version {}",
                self.schema_version
            ));
        }
        if self.tag == "HEAD" || self.tag.trim().is_empty() {
            return Err(miette::miette!(
                "Publishing requires a release tag, not HEAD"
            ));
        }
        if self.release_title.trim().is_empty() || self.release_body.trim().is_empty() {
            return Err(miette::miette!("Draft title and body cannot be empty"));
        }
        let parts: Vec<_> = self.repo.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|p| {
                p.is_empty()
                    || !p
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
            })
        {
            return Err(miette::miette!("Draft repo must be owner/repo"));
        }
        Ok(())
    }
}

pub const START: &str = "<!-- communique:start -->";
pub const END: &str = "<!-- communique:end -->";

pub fn merge_body(existing: &str, generated: &str) -> miette::Result<String> {
    if generated.contains(START) || generated.contains(END) {
        return Err(miette::miette!(
            "Generated body must not contain communique section markers"
        ));
    }
    let starts: Vec<_> = existing.match_indices(START).collect();
    let ends: Vec<_> = existing.match_indices(END).collect();
    match (starts.as_slice(), ends.as_slice()) {
        ([], []) => Ok(format!(
            "{}{}{}\n{}\n{}",
            existing,
            if existing.is_empty() { "" } else { "\n\n" },
            START,
            generated,
            END
        )),
        ([(start, _)], [(end, _)]) if start < end => Ok(format!(
            "{}\n{}\n{}",
            &existing[..start + START.len()],
            generated,
            &existing[*end..]
        )),
        _ => Err(miette::miette!(
            "Release must contain either no communique markers or exactly one ordered start/end pair"
        )),
    }
}

pub async fn publish_draft(path: &Path, dry_run: bool) -> miette::Result<()> {
    let draft: Draft = serde_json::from_str(&std::fs::read_to_string(path).into_diagnostic()?)
        .into_diagnostic()?;
    draft.validate()?;
    if dry_run && !draft.preserve_sections {
        println!("# {}\n\n{}", draft.release_title, draft.release_body);
        return Ok(());
    }
    let token = std::env::var("GITHUB_TOKEN")
        .into_diagnostic()
        .map_err(|_| miette::miette!("GITHUB_TOKEN is required to read or update the release"))?;
    let gh = GitHubClient::new(token, &draft.repo)?;
    publish_with_client(draft, dry_run, &gh).await
}

async fn publish_with_client(draft: Draft, dry_run: bool, gh: &GitHubClient) -> miette::Result<()> {
    draft.validate()?;
    let release = gh
        .get_release_by_tag(&draft.tag)
        .await?
        .ok_or_else(|| miette::miette!("No GitHub release found for {}", draft.tag))?;
    let body = if draft.preserve_sections {
        merge_body(
            release.body.as_deref().unwrap_or_default(),
            &draft.release_body,
        )?
    } else {
        draft.release_body
    };
    if dry_run {
        let title = if draft.preserve_sections {
            release.name.as_deref().unwrap_or(&draft.tag)
        } else {
            &draft.release_title
        };
        println!("# {title}\n\n{body}");
    } else {
        gh.update_release(
            release.id,
            &release.tag_name,
            if draft.preserve_sections {
                None
            } else {
                Some(&draft.release_title)
            },
            Some(&body),
        )
        .await?;
        eprintln!("Updated {}/{}", draft.repo, draft.tag);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_sends_exact_edited_content_and_preview_does_not_patch() {
        use wiremock::matchers::{body_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/releases/tags/v1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"id": 7, "tag_name": "v1", "name": "Existing", "body": "Old"}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/repos/owner/repo/releases/7"))
            .and(body_json(
                serde_json::json!({"tag_name": "v1", "name": "Edited title", "body": "Edited body\n\n"}),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": 7, "tag_name": "v1"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let gh = GitHubClient::with_base_url("token".into(), "owner/repo", server.uri()).unwrap();
        let notes = ParsedOutput {
            changelog: "Change".into(),
            release_title: "Edited title".into(),
            release_body: "Edited body\n\n".into(),
            usage: Default::default(),
            review: Default::default(),
        };
        publish_with_client(
            Draft::new("owner/repo", "v1", "v0", "abc", &notes, false),
            true,
            &gh,
        )
        .await
        .unwrap();
        publish_with_client(
            Draft::new("owner/repo", "v1", "v0", "abc", &notes, false),
            false,
            &gh,
        )
        .await
        .unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn publish_preserves_title_and_surrounding_sections() {
        use wiremock::matchers::{body_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let existing = format!("Manual intro\n{START}\nOld\n{END}\nManual footer");
        let expected = format!("Manual intro\n{START}\nNew\n{END}\nManual footer");
        Mock::given(method("GET")).and(path("/repos/owner/repo/releases/tags/v1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 7, "tag_name": "v1", "name": "Manual title", "body": existing})))
            .mount(&server).await;
        Mock::given(method("PATCH"))
            .and(path("/repos/owner/repo/releases/7"))
            .and(body_json(
                serde_json::json!({"tag_name": "v1", "body": expected}),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": 7, "tag_name": "v1"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let gh = GitHubClient::with_base_url("token".into(), "owner/repo", server.uri()).unwrap();
        let notes = ParsedOutput {
            changelog: "Change".into(),
            release_title: "Generated title".into(),
            release_body: "New".into(),
            usage: Default::default(),
            review: Default::default(),
        };
        publish_with_client(
            Draft::new("owner/repo", "v1", "v0", "abc", &notes, true),
            true,
            &gh,
        )
        .await
        .unwrap();
        publish_with_client(
            Draft::new("owner/repo", "v1", "v0", "abc", &notes, true),
            false,
            &gh,
        )
        .await
        .unwrap();
        server.verify().await;
    }

    #[test]
    fn marked_sections_preserve_manual_text_exactly() {
        let old = format!("Install instructions\n\n{START}\nOld notes\n{END}\n\nThanks!");
        assert_eq!(
            merge_body(&old, "New notes").unwrap(),
            format!("Install instructions\n\n{START}\nNew notes\n{END}\n\nThanks!")
        );
        assert_eq!(
            merge_body("Handwritten", "New").unwrap(),
            format!("Handwritten\n\n{START}\nNew\n{END}")
        );
        for bad in [
            format!("{START} orphan"),
            format!("{END}{START}"),
            format!("{START}{START}{END}"),
        ] {
            assert!(merge_body(&bad, "New").is_err());
        }
        assert!(merge_body("", START).is_err());
    }

    #[test]
    fn coverage_does_not_invent_missing_assessments() {
        let mut review = Review::from_submission(&serde_json::json!({"coverage": [
            {"commit": "abc", "status": "included", "reason": "Described in Added"},
            {"commit": "fake", "status": "included", "reason": "Not a real commit"}
        ]}));
        review.reconcile(
            "abc Feature\ndef Fix",
            vec![Coverage {
                commit: "ghi".into(),
                status: "omitted".into(),
                reason: "Label exclusion".into(),
            }],
        );
        assert_eq!(review.coverage.len(), 3);
        assert_eq!(review.coverage[1].status, "uncertain");
        assert_eq!(review.coverage[2].reason, "Label exclusion");
        assert!(!review.markdown("owner/repo", "v1", "v2").contains("fake"));
    }

    #[test]
    fn package_defaults_and_cli_overrides() {
        let config: crate::config::Config = toml::from_str(
            r#"
[packages.cli]
paths = ["crates/cli"]
tag_pattern = "cli/v*"
channel = "stable"
changelog_path = "crates/cli/CHANGELOG.md"
"#,
        )
        .unwrap();
        let mut options = WorkflowOptions {
            package: Some("cli".into()),
            tag_pattern: Some("cli/v2.*".into()),
            ..Default::default()
        };
        options.resolve(&config).unwrap();
        assert_eq!(options.paths, ["crates/cli"]);
        assert_eq!(options.tag_pattern.as_deref(), Some("cli/v2.*"));
        options.paths = vec!["../elsewhere".into()];
        assert!(options.resolve(&config).is_err());
        options.package = Some("missing".into());
        assert!(options.resolve(&config).is_err());
    }

    #[test]
    fn draft_roundtrip_keeps_editorial_changes() {
        let notes = ParsedOutput {
            changelog: "- Change".into(),
            release_title: "Title".into(),
            release_body: "Body\n\n".into(),
            usage: Default::default(),
            review: Default::default(),
        };
        let mut draft = Draft::new("owner/repo", "v1", "v0", "abc", &notes, false);
        draft.release_body = "Hand-edited body\n\n".into();
        let serialized = serde_json::to_string(&draft).unwrap();
        let decoded: Draft = serde_json::from_str(&serialized).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.release_body, "Hand-edited body\n\n");
        draft.schema_version = 99;
        assert!(draft.validate().is_err());
        draft.schema_version = 1;
        draft.tag = "HEAD".into();
        assert!(draft.validate().is_err());
    }
}
