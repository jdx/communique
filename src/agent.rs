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
