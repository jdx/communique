use std::path::Path;
use std::sync::Arc;

use clx::progress::ProgressJob;
use log::info;

use crate::anthropic::{AnthropicClient, ContentBlock, Message, MessagesRequest, ToolDefinition};
use crate::error::{Error, Result};
use crate::github::GitHubClient;
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
) -> Result<String> {
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

        let stop_reason = response.stop_reason.as_deref().unwrap_or("unknown");

        if tool_calls.is_empty() || stop_reason == "end_turn" {
            // Extract final text
            let text = response
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            if text.is_empty() {
                return Err(Error::Anthropic("no text in final response".into()));
            }
            return Ok(text);
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
