use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::error::{Error, Result};
use crate::llm::ToolDefinition;

pub fn definition() -> ToolDefinition {
    ToolDefinition {
        name: "read_file".into(),
        description:
            "Read the contents of a git-tracked file in the repository. Path is relative to the repo root."
                .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to repo root (must be tracked by git)"
                }
            },
            "required": ["path"]
        }),
    }
}

pub fn execute(repo_root: &Path, input: &serde_json::Value) -> Result<String> {
    let rel_path = input["path"]
        .as_str()
        .ok_or_else(|| Error::Tool("read_file: missing 'path' parameter".into()))?;

    // Sandbox: only permit reading files tracked by git. This excludes secrets
    // like .env, gitignored build artifacts, and .git internals.
    let tracked = Command::new("git")
        .args(["ls-files", "--error-unmatch", "--", rel_path])
        .current_dir(repo_root)
        .output()
        .map_err(|e| Error::Tool(format!("read_file: git ls-files: {e}")))?;
    if !tracked.status.success() {
        return Err(Error::Tool(format!(
            "read_file: {rel_path}: not a git-tracked file"
        )));
    }

    let full_path = repo_root.join(rel_path);
    let canonical = full_path
        .canonicalize()
        .map_err(|e| Error::Tool(format!("read_file: {rel_path}: {e}")))?;

    // Defense in depth: a tracked symlink could resolve outside the repo.
    let root_canonical = repo_root
        .canonicalize()
        .map_err(|e| Error::Tool(format!("read_file: cannot resolve repo root: {e}")))?;
    if !canonical.starts_with(&root_canonical) {
        return Err(Error::Tool(format!(
            "read_file: path escapes repo root: {rel_path}"
        )));
    }

    let contents = xx::file::read_to_string(&canonical)
        .map_err(|e| Error::Tool(format!("read_file: {rel_path}: {e}")))?;

    // Truncate very large files
    if contents.len() > 100_000 {
        Ok(format!(
            "{}...\n\n[file truncated at 100KB]",
            &contents[..100_000]
        ))
    } else {
        Ok(contents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::TempRepo;
    use serde_json::json;

    #[test]
    fn test_read_file_tracked() {
        let repo = TempRepo::new();
        repo.write_file("hello.txt", "world");
        repo.commit("init");

        let result = execute(repo.path(), &json!({"path": "hello.txt"})).unwrap();
        assert_eq!(result, "world");
    }

    #[test]
    fn test_read_file_untracked_rejected() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# hi");
        repo.commit("init");
        // .env exists on disk but is not tracked
        repo.write_file(".env", "SECRET=hunter2");

        let err = execute(repo.path(), &json!({"path": ".env"})).unwrap_err();
        assert!(
            err.to_string().contains("not a git-tracked file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_read_file_gitignored_rejected() {
        let repo = TempRepo::new();
        repo.write_file(".gitignore", "secrets.txt\n");
        repo.commit("init");
        repo.write_file("secrets.txt", "API_KEY=abc");

        let err = execute(repo.path(), &json!({"path": "secrets.txt"})).unwrap_err();
        assert!(
            err.to_string().contains("not a git-tracked file"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_read_file_path_traversal() {
        let repo = TempRepo::new();
        repo.write_file("README.md", "# hi");
        repo.commit("init");

        let err = execute(repo.path(), &json!({"path": "../secret.txt"})).unwrap_err();
        assert!(
            err.to_string().contains("not a git-tracked file"),
            "unexpected error: {err}"
        );
    }

    // Defense-in-depth: even when git tracks the path, a symlink resolving
    // outside the repo must be rejected by the canonicalize check.
    #[cfg(unix)]
    #[test]
    fn test_read_file_tracked_symlink_outside_repo_rejected() {
        use std::os::unix::fs::symlink;

        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, "sensitive").unwrap();

        let repo = TempRepo::new();
        symlink(&secret, repo.path().join("link")).unwrap();
        repo.commit("add symlink");

        let err = execute(repo.path(), &json!({"path": "link"})).unwrap_err();
        assert!(
            err.to_string().contains("escapes repo root"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_read_file_truncation() {
        let repo = TempRepo::new();
        repo.write_file("big.txt", &"x".repeat(200_000));
        repo.commit("init");

        let result = execute(repo.path(), &json!({"path": "big.txt"})).unwrap();
        assert!(result.contains("[file truncated at 100KB]"));
        assert!(result.len() < 200_000);
    }

    #[test]
    fn test_read_file_missing_path() {
        let repo = TempRepo::new();
        let err = execute(repo.path(), &json!({})).unwrap_err();
        assert!(err.to_string().contains("missing 'path'"));
    }
}
