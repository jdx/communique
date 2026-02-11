use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;

use crate::error::{Error, Result};

pub fn repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()?;
    if !output.status.success() {
        return Err(Error::Git("not a git repository".into()));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(PathBuf::from(path))
}

pub fn detect_remote(repo_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        return Err(Error::Git("no origin remote found".into()));
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
    Err(Error::Git(format!("cannot parse GitHub repo from remote URL: {url}")))
}

pub fn previous_tag(repo_root: &Path, current_tag: &str) -> Result<String> {
    // Get the tag before `current_tag` by listing tags sorted by version in descending order.
    let output = Command::new("git")
        .args(["tag", "--sort=-v:refname"])
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        return Err(Error::Git("failed to list tags".into()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let tags: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect();

    let mut found = false;
    for tag in &tags {
        if found {
            return Ok(tag.to_string());
        }
        if *tag == current_tag {
            found = true;
        }
    }
    Err(Error::Git(format!(
        "no previous tag found before {current_tag}"
    )))
}

pub fn log_between(repo_root: &Path, from: &str, to: &str) -> Result<String> {
    let range = format!("{from}..{to}");
    let output = Command::new("git")
        .args(["log", &range, "--pretty=format:%h %s", "--reverse"])
        .current_dir(repo_root)
        .output()?;
    if !output.status.success() {
        return Err(Error::Git(format!(
            "failed to get log for {range}: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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
        let log = "abc1234 feat: add feature (#123)\ndef5678 fix: bug (#456)\nghi9012 chore: update deps";
        assert_eq!(extract_pr_numbers(log), vec![123, 456]);
    }
}
