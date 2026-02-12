use std::path::Path;
use std::sync::Arc;

use clx::progress::ProgressJob;
use log::info;

use crate::anthropic::{AnthropicClient, ContentBlock, Message, MessagesRequest, ToolDefinition};
use crate::error::{Error, Result};
use crate::github::GitHubClient;
use crate::output::ParsedOutput;
use crate::tools;

const MAX_ITERATIONS: usize = 25;

pub async fn run(
    client: &AnthropicClient,
    system: &str,
    user_message: &str,
    tool_defs: Vec<ToolDefinition>,
    repo_root: &Path,
    github: Option<&GitHubClient>,
    job: &Arc<ProgressJob>,
) -> Result<ParsedOutput> {
    let mut messages = vec![Message {
        role: "user".into(),
        content: vec![ContentBlock::Text {
            text: user_message.into(),
        }],
    }];

    for iteration in 0..MAX_ITERATIONS {
        info!("agent iteration {}", iteration + 1);
        job.prop(
            "message",
            &format!("Thinking... (iteration {})", iteration + 1),
        );

        let request = MessagesRequest {
            model: client.model.clone(),
            max_tokens: client.max_tokens,
            system: system.into(),
            messages: messages.clone(),
            tools: tool_defs.clone(),
        };

        let response = client.send(&request).await?;

        info!(
            "usage: {} input, {} output tokens",
            response.usage.input_tokens, response.usage.output_tokens
        );

        // Collect tool calls from the response
        let tool_calls: Vec<_> = response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse { id, name, input } => {
                    Some((id.clone(), name.clone(), input.clone()))
                }
                _ => None,
            })
            .collect();

        // Add assistant message to history
        messages.push(Message {
            role: "assistant".into(),
            content: response.content.clone(),
        });

        // Check for submit_release_notes tool call — this is the final output
        for (_, name, input) in &tool_calls {
            if name == "submit_release_notes" {
                let changelog = input["changelog"]
                    .as_str()
                    .ok_or_else(|| Error::Parse("missing changelog in submission".into()))?
                    .to_string();
                let release_title = input["release_title"]
                    .as_str()
                    .ok_or_else(|| Error::Parse("missing release_title in submission".into()))?
                    .to_string();
                let release_body = input["release_body"]
                    .as_str()
                    .ok_or_else(|| Error::Parse("missing release_body in submission".into()))?
                    .to_string();
                return Ok(ParsedOutput {
                    changelog,
                    release_title,
                    release_body,
                });
            }
        }

        let stop_reason = response.stop_reason.as_deref().unwrap_or("unknown");

        if tool_calls.is_empty() || stop_reason == "end_turn" {
            return Err(Error::Anthropic(
                "model finished without calling submit_release_notes".into(),
            ));
        }

        // Execute tools and build results
        let mut results = Vec::new();
        for (id, name, input) in &tool_calls {
            info!("calling tool: {name}");
            job.prop("message", &format!("Running tool: {name}..."));
            match tools::dispatch(name, input, repo_root, github).await {
                Ok(output) => {
                    info!("tool {name}: {} bytes", output.len());
                    results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: output,
                        is_error: None,
                    });
                }
                Err(e) => {
                    info!("tool {name} error: {e}");
                    results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: format!("Error: {e}"),
                        is_error: Some(true),
                    });
                }
            }
        }

        messages.push(Message {
            role: "user".into(),
            content: results,
        });
    }

    Err(Error::Anthropic(format!(
        "agent loop exceeded {MAX_ITERATIONS} iterations"
    )))
}
