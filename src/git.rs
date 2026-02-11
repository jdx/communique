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

pub fn previous_tag(repo_root: &Path, current_tag: &str) -> Result<String> {
    let stdout = process::cmd("git", ["tag", "--sort=-v:refname"])
        .cwd(repo_root)
        .read()?;

    let mut found = false;
    for tag in stdout.lines().map(str::trim).filter(|t| !t.is_empty()) {
        if found {
            return Ok(tag.to_string());
        }
        if tag == current_tag {
            found = true;
        }
    }
    Err(Error::Git(format!(
        "no previous tag found before {current_tag}"
    )))
}

pub fn log_between(repo_root: &Path, from: &str, to: &str) -> Result<String> {
    let range = format!("{from}..{to}");
    let output = process::cmd("git", ["log", &range, "--pretty=format:%h %s", "--reverse"])
        .cwd(repo_root)
        .read()?;
    Ok(output)
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
