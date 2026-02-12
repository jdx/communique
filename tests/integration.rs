use std::path::Path;
use std::process::Command;

use serde_json::json;

/// Spin up a wiremock server that responds to OpenAI chat completions
/// with a `submit_release_notes` tool call, then run the full CLI
/// pipeline against a temporary git repo.
#[tokio::test]
async fn test_generate_end_to_end() {
    let server = wiremock::MockServer::start().await;

    // Mock the OpenAI chat completions endpoint
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/chat/completions"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "submit_release_notes",
                            "arguments": serde_json::to_string(&json!({
                                "changelog": "### Added\n- New feature",
                                "release_title": "First Release",
                                "release_body": "This is the first release.\n\n### Added\n- New feature"
                            })).unwrap()
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 100, "completion_tokens": 50}
        })))
        .mount(&server)
        .await;

    // Set up a temp git repo with commits and tags
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "test@test.com"]);
    git(repo, &["config", "user.name", "Test"]);

    std::fs::write(repo.join("README.md"), "# hello").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-m", "initial commit"]);
    git(repo, &["tag", "v0.1.0"]);

    std::fs::write(repo.join("src.rs"), "fn main() {}").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-m", "add feature"]);
    git(repo, &["tag", "v0.2.0"]);

    let output_file = repo.join("output.md");

    // Build the binary path
    let bin = env!("CARGO_BIN_EXE_communique");

    let result = Command::new(bin)
        .current_dir(repo)
        .args([
            "generate",
            "v0.2.0",
            "v0.1.0",
            "--dry-run",
            "--repo",
            "test/repo",
            "--provider",
            "openai",
            "--model",
            "test-model",
            "--base-url",
            &server.uri(),
            "--output",
            output_file.to_str().unwrap(),
        ])
        .env("OPENAI_API_KEY", "test-key")
        .env("CLX_NO_PROGRESS", "1")
        .output()
        .expect("failed to run communique");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        result.status.success(),
        "communique failed with status {}:\n{}",
        result.status,
        stderr
    );

    let output = std::fs::read_to_string(&output_file).expect("output file should exist");
    assert!(output.contains("First Release"), "output: {output}");
    assert!(
        output.contains("### Added"),
        "output should contain changelog sections: {output}"
    );
}

/// Same as above but with `--concise` to verify changelog-only output.
#[tokio::test]
async fn test_generate_concise() {
    let server = wiremock::MockServer::start().await;

    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/chat/completions"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "submit_release_notes",
                            "arguments": serde_json::to_string(&json!({
                                "changelog": "### Fixed\n- Bug fix",
                                "release_title": "Patch Release",
                                "release_body": "Fixed a bug.\n\n### Fixed\n- Bug fix"
                            })).unwrap()
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 50, "completion_tokens": 25}
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init"]);
    git(repo, &["config", "user.email", "test@test.com"]);
    git(repo, &["config", "user.name", "Test"]);

    std::fs::write(repo.join("README.md"), "# hello").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-m", "initial"]);
    git(repo, &["tag", "v1.0.0"]);

    std::fs::write(repo.join("fix.rs"), "// fix").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-m", "fix bug"]);
    git(repo, &["tag", "v1.0.1"]);

    let output_file = repo.join("output.md");
    let bin = env!("CARGO_BIN_EXE_communique");

    let result = Command::new(bin)
        .current_dir(repo)
        .args([
            "generate",
            "v1.0.1",
            "--dry-run",
            "--concise",
            "--repo",
            "test/repo",
            "--provider",
            "openai",
            "--model",
            "test-model",
            "--base-url",
            &server.uri(),
            "--output",
            output_file.to_str().unwrap(),
        ])
        .env("OPENAI_API_KEY", "test-key")
        .env("CLX_NO_PROGRESS", "1")
        .output()
        .expect("failed to run communique");

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "communique failed: {}", stderr);

    let output = std::fs::read_to_string(&output_file).unwrap();
    // Concise mode should output only the changelog, not the title
    assert!(output.contains("### Fixed"), "output: {output}");
    assert!(
        !output.contains("Patch Release"),
        "concise output should not contain release title: {output}"
    );
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}
