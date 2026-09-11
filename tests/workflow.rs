use serde_json::json;
use std::process::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn generate_edit_and_preview_draft_without_model_credentials() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    git(&["init"]);
    git(&["config", "user.name", "Test"]);
    git(&["config", "user.email", "test@example.com"]);
    std::fs::write(repo.join("file"), "old").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "Initial"]);
    git(&["tag", "v1.0.0"]);
    std::fs::write(repo.join("file"), "new").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "User feature"]);
    git(&["tag", "v2.0.0"]);
    let sha = git(&["rev-parse", "--short", "HEAD"]);
    Mock::given(method("POST")).and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": null, "tool_calls": [{
                "id": "submit", "type": "function", "function": {
                    "name": "submit_release_notes",
                    "arguments": serde_json::to_string(&json!({
                        "release_title": "New feature", "release_body": "A useful feature.",
                        "changelog": "## Added\n- A useful feature.",
                        "coverage": [{"commit": sha, "status": "included", "reason": "Described in Added."}],
                        "migration_guide": "# Upgrade\n\nNo migration is needed."
                    })).unwrap()
                }
            }]}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 10}
        }))).expect(1).mount(&server).await;
    let bin = env!("CARGO_BIN_EXE_communique");
    let result = Command::new(bin)
        .current_dir(repo)
        .args([
            "generate",
            "v2.0.0",
            "--repo",
            "owner/repo",
            "--provider",
            "openai",
            "--model",
            "test",
            "--base-url",
            &server.uri(),
            "--dry-run",
            "--draft",
            "draft.json",
            "--review-report",
            "review.md",
            "--migration-guide",
            "upgrade.md",
        ])
        .env_remove("GITHUB_TOKEN")
        .env("OPENAI_API_KEY", "test")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut draft: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo.join("draft.json")).unwrap()).unwrap();
    assert_eq!(draft["review"]["coverage"][0]["status"], "included");
    assert!(
        std::fs::read_to_string(repo.join("review.md"))
            .unwrap()
            .contains(&format!("/commit/{sha}"))
    );
    assert!(
        std::fs::read_to_string(repo.join("upgrade.md"))
            .unwrap()
            .contains("No migration")
    );
    draft["release_body"] = json!("Edited by maintainer.");
    std::fs::write(
        repo.join("draft.json"),
        serde_json::to_string(&draft).unwrap(),
    )
    .unwrap();
    let preview = Command::new(bin)
        .current_dir(dir.path().parent().unwrap())
        .arg("publish")
        .arg(repo.join("draft.json"))
        .arg("--dry-run")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("GITHUB_TOKEN")
        .output()
        .unwrap();
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(String::from_utf8_lossy(&preview.stdout).contains("Edited by maintainer."));
    server.verify().await;
}
