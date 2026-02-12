use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Mutex;

use serde_json::json;

use crate::error::Result;
use crate::llm::{
    Conversation, LlmClient, ToolCall, ToolDefinition, ToolResult, TurnResponse, Usage,
};

pub struct TempRepo {
    pub dir: tempfile::TempDir,
}

impl TempRepo {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init"]);
        git(p, &["config", "user.email", "test@test.com"]);
        git(p, &["config", "user.name", "Test"]);
        Self { dir }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn write_file(&self, name: &str, content: &str) {
        let path = self.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    pub fn commit(&self, msg: &str) {
        git(self.path(), &["add", "-A"]);
        git(self.path(), &["commit", "-m", msg]);
    }

    pub fn tag(&self, name: &str) {
        git(self.path(), &["tag", name]);
    }
}

fn git(dir: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
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

pub struct MockLlmClient {
    responses: Mutex<Vec<TurnResponse>>,
}

impl MockLlmClient {
    pub fn new(responses: Vec<TurnResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }
}

impl LlmClient for MockLlmClient {
    fn new_conversation(&self, _user_message: &str) -> Conversation {
        Conversation {
            messages: Vec::new(),
        }
    }

    fn append_tool_results(&self, _conversation: &mut Conversation, _results: &[ToolResult]) {}

    fn send_turn<'a>(
        &'a self,
        _system: &'a str,
        _conversation: &'a mut Conversation,
        _tools: &'a [ToolDefinition],
    ) -> Pin<Box<dyn Future<Output = Result<TurnResponse>> + Send + 'a>> {
        let resp = self.responses.lock().unwrap().remove(0);
        Box::pin(async move { Ok(resp) })
    }
}

pub fn submit_tool_call(changelog: &str, title: &str, body: &str) -> ToolCall {
    ToolCall {
        id: "call_1".into(),
        name: "submit_release_notes".into(),
        input: json!({
            "changelog": changelog,
            "release_title": title,
            "release_body": body,
        }),
    }
}

pub fn fake_usage() -> Usage {
    Usage {
        input_tokens: 0,
        output_tokens: 0,
    }
}
