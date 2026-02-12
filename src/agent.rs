use std::path::Path;
use std::sync::Arc;

use clx::progress::ProgressJob;
use log::info;

use crate::error::{Error, Result};
use crate::github::GitHubClient;
use crate::links;
use crate::llm::{LlmClient, StopReason, ToolDefinition, ToolResult};
use crate::output::ParsedOutput;
use crate::tools;

const MAX_ITERATIONS: usize = 25;

pub struct AgentContext<'a> {
    pub client: &'a dyn LlmClient,
    pub system: &'a str,
    pub user_message: &'a str,
    pub tool_defs: Vec<ToolDefinition>,
    pub repo_root: &'a Path,
    pub github: Option<&'a GitHubClient>,
    pub verify_links: bool,
    pub job: &'a Arc<ProgressJob>,
}

pub async fn run(ctx: AgentContext<'_>) -> Result<ParsedOutput> {
    let AgentContext {
        client,
        system,
        user_message,
        tool_defs,
        repo_root,
        github,
        verify_links,
        job,
    } = ctx;

    let mut conversation = client.new_conversation(user_message);

    for iteration in 0..MAX_ITERATIONS {
        info!("agent iteration {}", iteration + 1);
        job.prop(
            "message",
            &format!("Thinking... (iteration {})", iteration + 1),
        );

        let response = client
            .send_turn(system, &mut conversation, &tool_defs)
            .await?;

        info!(
            "usage: {} input, {} output tokens",
            response.usage.input_tokens, response.usage.output_tokens
        );

        // Check for submit_release_notes tool call — this is the final output
        let mut submit = None;
        for tc in &response.tool_calls {
            if tc.name == "submit_release_notes" {
                let changelog = tc.input["changelog"]
                    .as_str()
                    .ok_or_else(|| Error::Parse("missing changelog in submission".into()))?
                    .to_string();
                let release_title = tc.input["release_title"]
                    .as_str()
                    .ok_or_else(|| Error::Parse("missing release_title in submission".into()))?
                    .to_string();
                let release_body = tc.input["release_body"]
                    .as_str()
                    .ok_or_else(|| Error::Parse("missing release_body in submission".into()))?
                    .to_string();
                submit = Some((
                    tc.id.clone(),
                    ParsedOutput {
                        changelog,
                        release_title,
                        release_body,
                    },
                ));
            }
        }

        if let Some((tool_call_id, parsed)) = submit {
            if verify_links {
                job.prop("message", "Verifying links...");
                let broken = links::verify(&[&parsed.changelog, &parsed.release_body]).await;
                if !broken.is_empty() {
                    let summary = broken
                        .iter()
                        .map(|(url, reason)| format!("  {url} ({reason})"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    info!("broken links found, asking model to fix: {summary}");
                    client.append_tool_results(
                        &mut conversation,
                        &[ToolResult {
                            tool_call_id,
                            content: format!(
                                "The following links are broken:\n{summary}\n\n\
                                 Please fix or remove these URLs and call submit_release_notes again."
                            ),
                            is_error: true,
                        }],
                    );
                    continue;
                }
            }
            return Ok(parsed);
        }

        if response.tool_calls.is_empty() || response.stop_reason == StopReason::EndTurn {
            return Err(Error::Llm(
                "model finished without calling submit_release_notes".into(),
            ));
        }

        // Execute tools and build results
        let mut results = Vec::new();
        for tc in &response.tool_calls {
            let detail = tool_detail(&tc.name, &tc.input);
            info!("calling tool: {detail}");
            job.prop("message", &format!("Running tool: {detail}..."));
            match tools::dispatch(&tc.name, &tc.input, repo_root, github).await {
                Ok(output) => {
                    info!("tool {}: {} bytes", tc.name, output.len());
                    results.push(ToolResult {
                        tool_call_id: tc.id.clone(),
                        content: output,
                        is_error: false,
                    });
                }
                Err(e) => {
                    info!("tool {} error: {e}", tc.name);
                    results.push(ToolResult {
                        tool_call_id: tc.id.clone(),
                        content: format!("Error: {e}"),
                        is_error: true,
                    });
                }
            }
        }

        client.append_tool_results(&mut conversation, &results);
    }

    Err(Error::Llm(format!(
        "agent loop exceeded {MAX_ITERATIONS} iterations"
    )))
}

fn tool_detail(name: &str, input: &serde_json::Value) -> String {
    match name {
        "read_file" => {
            let path = input["path"].as_str().unwrap_or("?");
            format!("read_file({path})")
        }
        "list_files" => match input["glob"].as_str() {
            Some(glob) => format!("list_files({glob})"),
            None => "list_files".into(),
        },
        "grep" => {
            let pattern = input["pattern"].as_str().unwrap_or("?");
            format!("grep({pattern})")
        }
        "get_pr" => {
            let number = &input["number"];
            format!("get_pr(#{number})")
        }
        "get_pr_diff" => {
            let number = &input["number"];
            format!("get_pr_diff(#{number})")
        }
        _ => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use clx::progress::ProgressJobBuilder;
    use serde_json::json;

    use super::*;
    use crate::llm::{Conversation, StopReason, ToolCall, TurnResponse, Usage};

    struct MockLlmClient {
        responses: Mutex<Vec<TurnResponse>>,
    }

    impl MockLlmClient {
        fn new(responses: Vec<TurnResponse>) -> Self {
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

    fn submit_tool_call(changelog: &str, title: &str, body: &str) -> ToolCall {
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

    fn fake_usage() -> Usage {
        Usage {
            input_tokens: 0,
            output_tokens: 0,
        }
    }

    #[tokio::test]
    async fn test_direct_submission() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![submit_tool_call("log", "v1.0", "body")],
            stop_reason: StopReason::ToolUse,
            usage: fake_usage(),
        }]);
        let job = Arc::new(ProgressJobBuilder::new().build());
        let tmp = std::env::temp_dir();
        let ctx = AgentContext {
            client: &client,
            system: "",
            user_message: "",
            tool_defs: vec![],
            repo_root: &tmp,
            github: None,
            verify_links: false,
            job: &job,
        };
        let result = run(ctx).await.unwrap();
        assert_eq!(result.changelog, "log");
        assert_eq!(result.release_title, "v1.0");
        assert_eq!(result.release_body, "body");
    }

    #[tokio::test]
    async fn test_tool_use_then_submission() {
        let client = MockLlmClient::new(vec![
            TurnResponse {
                tool_calls: vec![ToolCall {
                    id: "call_0".into(),
                    name: "read_file".into(),
                    input: json!({"path": "README.md"}),
                }],
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            TurnResponse {
                tool_calls: vec![submit_tool_call("changes", "v2.0", "notes")],
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
        ]);
        let job = Arc::new(ProgressJobBuilder::new().build());
        let tmp = std::env::temp_dir();
        let ctx = AgentContext {
            client: &client,
            system: "",
            user_message: "",
            tool_defs: vec![],
            repo_root: &tmp,
            github: None,
            verify_links: false,
            job: &job,
        };
        let result = run(ctx).await.unwrap();
        assert_eq!(result.changelog, "changes");
        assert_eq!(result.release_title, "v2.0");
        assert_eq!(result.release_body, "notes");
    }

    #[tokio::test]
    async fn test_end_turn_without_submission() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: fake_usage(),
        }]);
        let job = Arc::new(ProgressJobBuilder::new().build());
        let tmp = std::env::temp_dir();
        let ctx = AgentContext {
            client: &client,
            system: "",
            user_message: "",
            tool_defs: vec![],
            repo_root: &tmp,
            github: None,
            verify_links: false,
            job: &job,
        };
        let err = run(ctx).await.unwrap_err();
        assert!(matches!(err, Error::Llm(_)));
        assert!(err.to_string().contains("without calling"));
    }

    #[tokio::test]
    async fn test_empty_tool_calls() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![],
            stop_reason: StopReason::ToolUse,
            usage: fake_usage(),
        }]);
        let job = Arc::new(ProgressJobBuilder::new().build());
        let tmp = std::env::temp_dir();
        let ctx = AgentContext {
            client: &client,
            system: "",
            user_message: "",
            tool_defs: vec![],
            repo_root: &tmp,
            github: None,
            verify_links: false,
            job: &job,
        };
        let err = run(ctx).await.unwrap_err();
        assert!(matches!(err, Error::Llm(_)));
    }

    #[tokio::test]
    async fn test_max_iterations_exceeded() {
        let responses: Vec<TurnResponse> = (0..MAX_ITERATIONS + 1)
            .map(|i| TurnResponse {
                tool_calls: vec![ToolCall {
                    id: format!("call_{i}"),
                    name: "read_file".into(),
                    input: json!({"path": "f.txt"}),
                }],
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            })
            .collect();
        let client = MockLlmClient::new(responses);
        let job = Arc::new(ProgressJobBuilder::new().build());
        let tmp = std::env::temp_dir();
        let ctx = AgentContext {
            client: &client,
            system: "",
            user_message: "",
            tool_defs: vec![],
            repo_root: &tmp,
            github: None,
            verify_links: false,
            job: &job,
        };
        let err = run(ctx).await.unwrap_err();
        assert!(matches!(err, Error::Llm(_)));
        assert!(err.to_string().contains("exceeded"));
    }
}
