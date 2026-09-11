use std::path::{Path, PathBuf};

use regex::Regex;
use xx::git::Git;
use xx::process;

use crate::error::{Error, Result};

pub fn repo_root() -> Result<PathBuf> {
    let path = process::cmd("git", ["rev-parse", "--show-toplevel"]).read()?;
    Ok(PathBuf::from(path))
}

pub fn detect_remote(repo_root: &Path) -> Result<String> {
    let git = Git::new(repo_root.to_path_buf());
    let url = git
        .get_remote_url()
        .ok_or_else(|| Error::Git("no origin remote found".into()))?;
    parse_owner_repo(&url)
}

fn parse_owner_repo(url: &str) -> Result<String> {
    // SSH: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let repo = rest.trim_end_matches(".git");
        return Ok(repo.to_string());
    }
    // HTTPS: https://github.com/owner/repo.git
    if let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    {
        let repo = rest.trim_end_matches(".git");
        return Ok(repo.to_string());
    }
    Err(Error::Git(format!(
        "cannot parse GitHub repo from remote URL: {url}"
    )))
}

#[cfg(test)]
pub fn previous_tag(repo_root: &Path, current_tag: &str) -> Result<String> {
    previous_tag_filtered(repo_root, current_tag, None, crate::workflow::Channel::All)
}

pub fn previous_tag_filtered(
    repo_root: &Path,
    current_tag: &str,
    pattern: Option<&str>,
    channel: crate::workflow::Channel,
) -> Result<String> {
    let target = resolve_ref(repo_root, current_tag)?;
    let stdout = process::cmd(
        "git",
        [
            "tag",
            "--merged",
            &target,
            "--sort=-v:refname",
            "--list",
            pattern.unwrap_or("*"),
        ],
    )
    .cwd(repo_root)
    .read()?;
    let target_is_tag = stdout.lines().any(|tag| tag.trim() == current_tag);
    let stable = matches!(channel, crate::workflow::Channel::Stable);
    let prerelease = Regex::new(r"[0-9]+\.[0-9]+(?:\.[0-9]+)?-").unwrap();
    for tag in stdout.lines().map(str::trim).filter(|tag| !tag.is_empty()) {
        if tag == current_tag || (stable && prerelease.is_match(tag)) {
            continue;
        }
        // A different tag on the target commit is not a useful baseline.
        if target_is_tag && verify_ref(repo_root, tag)? == target {
            continue;
        }
        return Ok(tag.into());
    }
    root_commit(repo_root, &target)
}

pub fn verify_ref(repo_root: &Path, git_ref: &str) -> Result<String> {
    let commit = format!("{git_ref}^{{commit}}");
    process::cmd(
        "git",
        ["rev-parse", "--verify", "--end-of-options", &commit],
    )
    .cwd(repo_root)
    .stderr_capture()
    .read()
    .map(|s| s.trim().to_string())
    .map_err(|_| Error::Git(format!("Cannot resolve git reference '{git_ref}'")))
}

fn root_commit(repo_root: &Path, target: &str) -> Result<String> {
    let sha = process::cmd("git", ["rev-list", "--max-parents=0", target])
        .cwd(repo_root)
        .read()?;
    sha.lines()
        .next()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| Error::Git("no commits found".into()))
}

/// Resolve a ref to a commit, falling back to HEAD if the ref doesn't exist
/// (e.g. a version tag that hasn't been created yet).
pub fn resolve_ref(repo_root: &Path, git_ref: &str) -> Result<String> {
    verify_ref(repo_root, git_ref).or_else(|_| verify_ref(repo_root, "HEAD"))
}

#[cfg(test)]
pub fn log_between(repo_root: &Path, from: &str, to: &str) -> Result<String> {
    log_between_paths(repo_root, from, to, &[])
}

pub fn log_between_paths(
    repo_root: &Path,
    from: &str,
    to: &str,
    paths: &[String],
) -> Result<String> {
    let from = verify_ref(repo_root, from)?;
    let to = resolve_ref(repo_root, to)?;
    let range = format!("{from}..{to}");
    let mut args = vec![
        "log".to_string(),
        range,
        "--pretty=format:%h %s".into(),
        "--reverse".into(),
        "--".into(),
    ];
    args.extend(paths.iter().map(|p| format!(":(literal){p}")));
    process::cmd("git", args)
        .cwd(repo_root)
        .read()
        .map_err(Into::into)
}

pub fn extract_pr_numbers(log: &str) -> Vec<u64> {
    let re = Regex::new(r"\(#(\d+)\)").unwrap();
    re.captures_iter(log)
        .filter_map(|cap| cap[1].parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_respects_ancestry_pattern_and_channel() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("cli/file", "a");
        repo.commit("base");
        repo.tag("cli/v1.0.0");
        repo.write_file("cli/file", "b");
        repo.commit("beta");
        repo.tag("cli/v2.0.0-beta.1");
        repo.write_file("cli/file", "c");
        repo.commit("target");
        repo.tag("cli/v2.0.0");
        repo.write_file("cli/file", "d");
        repo.commit("future");
        repo.tag("cli/v3.0.0");
        assert_eq!(
            previous_tag_filtered(
                repo.path(),
                "cli/v2.0.0",
                Some("cli/*"),
                crate::workflow::Channel::Stable
            )
            .unwrap(),
            "cli/v1.0.0"
        );
        assert_eq!(
            previous_tag_filtered(
                repo.path(),
                "cli/v2.0.0",
                Some("cli/*"),
                crate::workflow::Channel::All
            )
            .unwrap(),
            "cli/v2.0.0-beta.1"
        );
    }

    #[test]
    fn path_filter_omits_other_packages_and_rejects_bad_baseline() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("cli/file", "a");
        repo.commit("base");
        repo.tag("v1");
        repo.write_file("server/file", "a");
        repo.commit("server feature");
        repo.write_file("cli/file", "b");
        repo.commit("cli feature");
        repo.tag("v2");
        let log = log_between_paths(repo.path(), "v1", "v2", &["cli".into()]).unwrap();
        assert!(log.contains("cli feature"));
        assert!(!log.contains("server feature"));
        assert!(log_between_paths(repo.path(), "typo", "v2", &[]).is_err());
    }

    #[test]
    fn test_parse_owner_repo_ssh() {
        assert_eq!(
            parse_owner_repo("git@github.com:jdx/pitchfork.git").unwrap(),
            "jdx/pitchfork"
        );
    }

    #[test]
    fn test_parse_owner_repo_https() {
        assert_eq!(
            parse_owner_repo("https://github.com/jdx/pitchfork.git").unwrap(),
            "jdx/pitchfork"
        );
    }

    #[test]
    fn test_extract_pr_numbers() {
        let log =
            "abc1234 feat: add feature (#123)\ndef5678 fix: bug (#456)\nghi9012 chore: update deps";
        assert_eq!(extract_pr_numbers(log), vec![123, 456]);
    }

    #[test]
    fn test_resolve_ref_existing() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("f.txt", "a");
        repo.commit("first");
        repo.tag("v1.0.0");

        let sha = resolve_ref(repo.path(), "v1.0.0").unwrap();
        assert!(!sha.is_empty());
        assert_eq!(sha.len(), 40);
    }

    #[test]
    fn test_resolve_ref_fallback_to_head() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("f.txt", "a");
        repo.commit("first");

        let sha = resolve_ref(repo.path(), "nonexistent-tag").unwrap();
        assert_eq!(sha.len(), 40);
    }

    #[test]
    fn test_previous_tag() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("f.txt", "a");
        repo.commit("first");
        repo.tag("v1.0.0");
        repo.write_file("f.txt", "b");
        repo.commit("second");
        repo.tag("v2.0.0");

        let prev = previous_tag(repo.path(), "v2.0.0").unwrap();
        assert_eq!(prev, "v1.0.0");
    }

    #[test]
    fn test_previous_tag_first_release() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("f.txt", "a");
        repo.commit("first");
        repo.tag("v1.0.0");

        // No previous tag — should fall back to root commit SHA
        let prev = previous_tag(repo.path(), "v1.0.0").unwrap();
        assert_eq!(prev.len(), 40);
    }

    #[test]
    fn test_log_between() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("f.txt", "a");
        repo.commit("first commit");
        repo.tag("v1.0.0");
        repo.write_file("f.txt", "b");
        repo.commit("second commit");
        repo.tag("v2.0.0");

        let log = log_between(repo.path(), "v1.0.0", "v2.0.0").unwrap();
        assert!(log.contains("second commit"));
        assert!(!log.contains("first commit"));
    }
}
