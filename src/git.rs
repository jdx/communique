use std::path::{Path, PathBuf};

use regex::Regex;
use xx::git::Git;
use xx::process;

use crate::error::{Error, Result};

pub fn repo_root() -> Result<PathBuf> {
    let path = process::cmd("git", ["rev-parse", "--show-toplevel"]).read()?;
    Ok(PathBuf::from(path))
}

/// URL of the `origin` remote, if any.
pub fn remote_url(repo_root: &Path) -> Option<String> {
    Git::new(repo_root.to_path_buf()).get_remote_url()
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
    // In a shallow clone the earlier tags may simply not have been fetched,
    // and the "root" is the shallow boundary, often the target itself, which
    // would make an empty range.
    if is_shallow(repo_root)? {
        return Err(Error::Git(format!(
            "no earlier tag is reachable from '{current_tag}' because this is a shallow clone. \
             Fetch the full history and tags (in GitHub Actions, `fetch-depth: 0` on \
             actions/checkout), or pass the previous tag as the second argument"
        )));
    }
    root_commit(repo_root, &target)
}

fn is_shallow(repo_root: &Path) -> Result<bool> {
    let stdout = process::cmd("git", ["rev-parse", "--is-shallow-repository"])
        .cwd(repo_root)
        .read()?;
    Ok(stdout.trim() == "true")
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

/// GitHub logins credited on one commit: its author and any `Co-authored-by`
/// trailers, in that order, without duplicates.
pub type CommitCredit = (String, Vec<String>);

/// List the GitHub logins credited on each commit in `from..to`, so release
/// notes can credit people who appear only as a commit author or co-author
/// (for example a contributor whose PR was recreated by a maintainer).
pub fn commit_credits(
    repo_root: &Path,
    from: &str,
    to: &str,
    paths: &[String],
) -> Result<Vec<CommitCredit>> {
    let from = verify_ref(repo_root, from)?;
    let to = resolve_ref(repo_root, to)?;
    let mut args = vec![
        "log".to_string(),
        format!("{from}..{to}"),
        "--pretty=format:%h%x1f%ae%x1f%(trailers:key=Co-authored-by,valueonly,unfold,separator=%x1e)%x1d"
            .into(),
        "--reverse".into(),
        "--".into(),
    ];
    args.extend(paths.iter().map(|p| format!(":(literal){p}")));
    let raw = process::cmd("git", args).cwd(repo_root).read()?;
    Ok(parse_commit_credits(&raw))
}

fn parse_commit_credits(raw: &str) -> Vec<CommitCredit> {
    raw.split('\x1d')
        .filter_map(|record| {
            let mut fields = record.trim().splitn(3, '\x1f');
            let hash = fields.next().filter(|h| !h.is_empty())?;
            let author = fields.next().unwrap_or_default();
            let co_authors = fields.next().unwrap_or_default();
            let mut logins: Vec<String> = Vec::new();
            for login in std::iter::once(author)
                .chain(co_authors.split('\x1e'))
                .filter_map(login_from_identity)
            {
                if !logins.iter().any(|l| l.eq_ignore_ascii_case(&login)) {
                    logins.push(login);
                }
            }
            (!logins.is_empty()).then(|| (hash.to_string(), logins))
        })
        .collect()
}

/// Extract a GitHub login from an email or a `Name <email>` trailer value.
/// Only GitHub noreply addresses (`<id>+<login>@users.noreply.github.com` or
/// `<login>@users.noreply.github.com`) identify a login; bots are skipped.
fn login_from_identity(identity: &str) -> Option<String> {
    let email = identity
        .rsplit_once('<')
        .map_or(identity, |(_, rest)| rest.trim_end_matches('>'))
        .trim();
    let local = email.strip_suffix("@users.noreply.github.com")?;
    let login = local.rsplit_once('+').map_or(local, |(_, login)| login);
    (is_valid_login(login) && !is_automation_login(login)).then(|| login.to_string())
}

/// GitHub logins are 1-39 ASCII letters, digits and hyphens; Enterprise Managed
/// User logins (`name_shortcode`) also contain an underscore. `[bot]` accounts
/// fail this check on purpose; they are automation and are never credited.
fn is_valid_login(login: &str) -> bool {
    (1..=39).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// AI assistants and automation are tools, not contributors; never credit them.
fn is_automation_login(login: &str) -> bool {
    const SKIPPED: &[&str] = &[
        "claude",
        "copilot",
        "codex",
        "cursoragent",
        "github-actions",
        "dependabot",
        "renovate",
    ];
    let login = login.to_ascii_lowercase();
    login.ends_with("[bot]") || SKIPPED.contains(&login.as_str())
}

/// GitLab merge requests referenced by one commit: `(short hash, full hash, numbers)`.
pub type MergeRequestRefs = (String, String, Vec<u64>);

/// Find the GitLab merge requests each commit in `from..to` mentions, either
/// as `!123` (for example a `(!123)` subject suffix) or in GitLab's default
/// merge commit trailer `See merge request group/project!123`. References to
/// other projects are ignored. Commits without a reference are returned with
/// an empty list so callers can look them up through the API.
pub fn merge_request_refs(
    repo_root: &Path,
    from: &str,
    to: &str,
    paths: &[String],
    project_path: &str,
) -> Result<Vec<MergeRequestRefs>> {
    let from = verify_ref(repo_root, from)?;
    let to = resolve_ref(repo_root, to)?;
    let mut args = vec![
        "log".to_string(),
        format!("{from}..{to}"),
        // `-z` ends each record with NUL, which a commit message cannot contain.
        "-z".into(),
        "--pretty=format:%h%x1f%H%x1f%B".into(),
        "--reverse".into(),
        "--".into(),
    ];
    args.extend(paths.iter().map(|p| format!(":(literal){p}")));
    let raw = process::cmd("git", args).cwd(repo_root).read()?;
    Ok(raw
        .split('\0')
        .filter_map(|record| {
            let is_hash = |h: &&str| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit());
            let mut fields = record.trim_start().splitn(3, '\x1f');
            let short = fields.next().filter(is_hash)?;
            let full = fields.next().filter(is_hash)?;
            let message = fields.next().unwrap_or_default();
            Some((
                short.to_string(),
                full.to_string(),
                extract_mr_numbers(message, project_path),
            ))
        })
        .collect())
}

fn extract_mr_numbers(message: &str, project_path: &str) -> Vec<u64> {
    let re = Regex::new(r"(?:^|[\s(\[])((?:[\w.-]+/)+[\w.-]+)?!(\d+)\b").unwrap();
    let mut numbers = Vec::new();
    for cap in re.captures_iter(message) {
        if cap
            .get(1)
            .is_some_and(|p| !p.as_str().eq_ignore_ascii_case(project_path))
        {
            continue;
        }
        if let Ok(n) = cap[2].parse()
            && !numbers.contains(&n)
        {
            numbers.push(n);
        }
    }
    numbers
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
    fn extracts_gitlab_merge_request_references() {
        assert_eq!(
            extract_mr_numbers(
                "Merge branch 'feat' into 'main'\n\nAdd widgets (!12)\n\nSee merge request group/proj!14",
                "group/proj"
            ),
            vec![12, 14]
        );
        assert_eq!(
            extract_mr_numbers(
                "See merge request other/proj!3\nfeat!: breaking",
                "group/proj"
            ),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn merge_request_refs_reads_commit_bodies() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("f", "a");
        repo.commit("base");
        repo.tag("v1");
        repo.write_file("f", "b");
        repo.commit("Merge branch 'x' into 'main'\n\nSee merge request group/proj!7");
        repo.write_file("f", "c");
        repo.commit("fix: squashed change");
        repo.write_file("f", "d");
        repo.commit("fix: odd body\n\nseparators \x1d and \x1f inside (!9)");
        let refs = merge_request_refs(repo.path(), "v1", "HEAD", &[], "group/proj").unwrap();
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].2, vec![7]);
        assert_eq!(refs[0].1.len(), 40);
        assert!(refs[1].2.is_empty());
        assert_eq!(refs[2].2, vec![9]);
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
    fn test_previous_tag_shallow_clone_without_earlier_tags() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("f.txt", "0");
        repo.commit("root");
        repo.write_file("f.txt", "a");
        repo.commit("first");
        repo.tag("v1.0.0");
        repo.write_file("f.txt", "b");
        repo.commit("second");
        repo.tag("v2.0.0");

        // What actions/checkout does by default: only the tag, one commit deep.
        let clone = tempfile::tempdir().unwrap();
        let source = format!("file://{}", repo.path().display());
        let status = std::process::Command::new("git")
            .args(["clone", "-q", "--depth", "1", "--branch", "v2.0.0", &source])
            .arg(clone.path())
            .status()
            .unwrap();
        assert!(status.success());

        let err = previous_tag(clone.path(), "v2.0.0").unwrap_err();
        assert!(err.to_string().contains("shallow clone"), "{err}");

        // A shallow clone deep enough to reach the earlier tag still works. Two
        // commits deep reaches v1.0.0 (fetched along with its commit) but not the
        // root commit, so the clone stays shallow.
        let deeper = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git")
            .args(["clone", "-q", "--depth", "2", "--branch", "v2.0.0", &source])
            .arg(deeper.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(is_shallow(deeper.path()).unwrap());
        let status = std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", "v1.0.0", "v2.0.0"])
            .current_dir(deeper.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(previous_tag(deeper.path(), "v2.0.0").unwrap(), "v1.0.0");
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

    #[test]
    fn login_from_identity_accepts_only_github_noreply() {
        assert_eq!(
            login_from_identity("Navid EMAD <66868650+navidemad@users.noreply.github.com>"),
            Some("navidemad".into())
        );
        assert_eq!(
            login_from_identity("octo@users.noreply.github.com"),
            Some("octo".into())
        );
        assert_eq!(
            login_from_identity("Ada <ada,bob@users.noreply.github.com>"),
            None
        );
        assert_eq!(
            login_from_identity(&format!("{}@users.noreply.github.com", "a".repeat(40))),
            None
        );
        assert_eq!(
            login_from_identity("Ann <9+ann_acme@users.noreply.github.com>"),
            Some("ann_acme".into())
        );
        assert_eq!(login_from_identity("Claude <noreply@anthropic.com>"), None);
        assert_eq!(
            login_from_identity("bot <1+renovate[bot]@users.noreply.github.com>"),
            None
        );
        assert_eq!(
            login_from_identity("Copilot <175728472+Copilot@users.noreply.github.com>"),
            None
        );
    }

    #[test]
    fn commit_credits_include_author_and_coauthor_trailers() {
        let repo = crate::test_helpers::TempRepo::new();
        repo.write_file("a", "a");
        repo.commit("base");
        repo.tag("v1");
        repo.write_file("a", "b");
        repo.commit_as(
            "fix: recreated (#1)\n\nCo-authored-by: Ada <7+ada@users.noreply.github.com>\nCo-authored-by: Claude <noreply@anthropic.com>\nCo-authored-by: Ada <7+ada@users.noreply.github.com>",
            "Maintainer <1+Maint@users.noreply.github.com>",
        );
        repo.write_file("a", "c");
        repo.commit_as("chore: plain", "Bob <2+bob@users.noreply.github.com>");
        repo.write_file("a", "d");
        repo.commit_as("chore: ignored", "Test <test@test.com>");
        let credits = commit_credits(repo.path(), "v1", "HEAD", &[]).unwrap();
        let logins: Vec<_> = credits.iter().map(|(_, l)| l.clone()).collect();
        assert_eq!(
            logins,
            vec![
                vec!["Maint".to_string(), "ada".into()],
                vec!["bob".to_string()]
            ]
        );
    }

    #[test]
    fn parse_commit_credits_dedupes_logins_ignoring_case() {
        let raw =
            "abc\x1f7+Ada@users.noreply.github.com\x1fAda <7+ada@users.noreply.github.com>\x1d";
        assert_eq!(
            parse_commit_credits(raw),
            vec![("abc".to_string(), vec!["Ada".to_string()])]
        );
    }
}
