use std::path::Path;
use std::sync::Arc;

use clx::progress::ProgressJob;
use log::info;

use crate::error::{Error, Result};
use crate::github::GitHubClient;
use crate::links;
use crate::llm::{LlmClient, StopReason, ToolDefinition, ToolResult, Usage};
use crate::output::{self, ParsedOutput};
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
    let mut cache = tools::ToolCache::new();
    let mut total_usage = Usage::default();

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
        total_usage += response.usage.clone();

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
                        usage: total_usage.clone(),
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
            // Fallback: try to parse text content as release notes
            if let Some(text) = &response.text
                && let Some(parsed) = output::parse_text_fallback(text)
            {
                log::warn!("model did not call submit_release_notes; falling back to text parsing");
                return Ok(parsed);
            }
            return Err(Error::Llm(
                "model finished without calling submit_release_notes".into(),
            ));
        }

        // Execute tools: use cache for repeated calls, dispatch uncached concurrently
        let details: Vec<_> = response
            .tool_calls
            .iter()
            .map(|tc| tool_detail(&tc.name, &tc.input))
            .collect();
        for detail in &details {
            info!("calling tool: {detail}");
        }

        // Snapshot cache hits before dispatching
        let cache_hits: Vec<Option<String>> = response
            .tool_calls
            .iter()
            .map(|tc| cache.get(&tc.name, &tc.input).map(|s| s.to_string()))
            .collect();
        let hit_count = cache_hits.iter().filter(|c| c.is_some()).count();
        let dispatch_count = response.tool_calls.len() - hit_count;

        if hit_count > 0 {
            info!("{hit_count} tool call(s) served from cache");
        }
        if dispatch_count > 0 {
            job.prop(
                "message",
                &format!(
                    "Running {} tool{}...{}",
                    dispatch_count,
                    if dispatch_count == 1 { "" } else { "s" },
                    if hit_count > 0 {
                        format!(" ({hit_count} cached)")
                    } else {
                        String::new()
                    }
                ),
            );
        }

        // Dispatch only uncached tool calls concurrently
        let dispatch_indices: Vec<usize> = cache_hits
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_none())
            .map(|(i, _)| i)
            .collect();
        let futures: Vec<_> = dispatch_indices
            .iter()
            .map(|&i| {
                let tc = &response.tool_calls[i];
                tools::dispatch(&tc.name, &tc.input, repo_root, github)
            })
            .collect();
        let dispatch_outcomes = futures_util::future::join_all(futures).await;

        // Merge cached + dispatched results in original order
        let mut dispatch_iter = dispatch_outcomes.into_iter();
        let results: Vec<_> = response
            .tool_calls
            .iter()
            .zip(cache_hits)
            .map(|(tc, cached)| {
                if let Some(content) = cached {
                    ToolResult {
                        tool_call_id: tc.id.clone(),
                        content,
                        is_error: false,
                    }
                } else {
                    match dispatch_iter.next().unwrap() {
                        Ok(output) => {
                            info!("tool {}: {} bytes", tc.name, output.len());
                            cache.insert(&tc.name, &tc.input, output.clone());
                            ToolResult {
                                tool_call_id: tc.id.clone(),
                                content: output,
                                is_error: false,
                            }
                        }
                        Err(e) => {
                            info!("tool {} error: {e}", tc.name);
                            ToolResult {
                                tool_call_id: tc.id.clone(),
                                content: format!("Error: {e}"),
                                is_error: true,
                            }
                        }
                    }
                }
            })
            .collect();

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
    use std::sync::Arc;

    use clx::progress::ProgressJobBuilder;
    use serde_json::json;

    use super::*;
    use crate::llm::{StopReason, ToolCall, TurnResponse};
    use crate::test_helpers::{MockLlmClient, fake_usage, submit_tool_call};

    #[tokio::test]
    async fn test_direct_submission() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![submit_tool_call("log", "v1.0", "body")],
            text: None,
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
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            TurnResponse {
                tool_calls: vec![submit_tool_call("changes", "v2.0", "notes")],
                text: None,
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
            text: None,
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
    async fn test_end_turn_text_fallback() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![],
            text: Some("# Cool Release\n\nSome great changes\n- Added X".into()),
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
        let result = run(ctx).await.unwrap();
        assert_eq!(result.release_title, "Cool Release");
        assert_eq!(result.release_body, "Some great changes\n- Added X");
    }

    #[tokio::test]
    async fn test_empty_tool_calls() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![],
            text: None,
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
    async fn test_missing_changelog_field() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "submit_release_notes".into(),
                input: json!({
                    "release_title": "v1.0",
                    "release_body": "body",
                }),
            }],
            text: None,
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
        assert!(matches!(err, Error::Parse(_)));
        assert!(err.to_string().contains("changelog"));
    }

    #[tokio::test]
    async fn test_missing_release_title_field() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "submit_release_notes".into(),
                input: json!({
                    "changelog": "log",
                    "release_body": "body",
                }),
            }],
            text: None,
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
        assert!(matches!(err, Error::Parse(_)));
        assert!(err.to_string().contains("release_title"));
    }

    #[tokio::test]
    async fn test_missing_release_body_field() {
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "submit_release_notes".into(),
                input: json!({
                    "changelog": "log",
                    "release_title": "v1.0",
                }),
            }],
            text: None,
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
        assert!(matches!(err, Error::Parse(_)));
        assert!(err.to_string().contains("release_body"));
    }

    #[tokio::test]
    async fn test_verify_links_pass_on_first_try() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .respond_with(wiremock::ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let url = format!("{}/valid", server.uri());
        let client = MockLlmClient::new(vec![TurnResponse {
            tool_calls: vec![ToolCall {
                id: "call_1".into(),
                name: "submit_release_notes".into(),
                input: json!({
                    "changelog": "changes",
                    "release_title": "v1.0",
                    "release_body": format!("See {url}"),
                }),
            }],
            text: None,
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
            verify_links: true,
            job: &job,
        };
        let result = run(ctx).await.unwrap();
        assert_eq!(result.changelog, "changes");
        assert_eq!(result.release_body, format!("See {url}"));
    }

    #[tokio::test]
    async fn test_verify_links_retry() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("HEAD"))
            .respond_with(wiremock::ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let broken_url = format!("{}/broken", server.uri());
        let client = MockLlmClient::new(vec![
            // First: submit with broken link
            TurnResponse {
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "submit_release_notes".into(),
                    input: json!({
                        "changelog": "changes",
                        "release_title": "v1.0",
                        "release_body": format!("See {broken_url}"),
                    }),
                }],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            // Second: submit without broken link
            TurnResponse {
                tool_calls: vec![submit_tool_call("changes", "v1.0", "Fixed notes")],
                text: None,
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
            verify_links: true,
            job: &job,
        };
        let result = run(ctx).await.unwrap();
        assert_eq!(result.release_body, "Fixed notes");
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
                text: None,
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
