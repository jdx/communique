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
const MAX_MALFORMED_SUBMISSIONS: usize = 3;

fn parse_submission(input: &serde_json::Value, usage: &Usage) -> Result<ParsedOutput> {
    let changelog = field_as_string(input, "changelog")?;
    let release_title = field_as_string(input, "release_title")?;
    let release_body = field_as_string(input, "release_body")?;

    Ok(ParsedOutput {
        changelog,
        release_title,
        release_body,
        usage: usage.clone(),
    })
}

fn field_as_string(input: &serde_json::Value, field: &str) -> Result<String> {
    let value = &input[field];
    if value.is_null() {
        return Err(Error::Parse(format!("missing `{field}`")));
    }
    match value.as_str() {
        Some(s) if !s.trim().is_empty() => Ok(s.to_string()),
        Some(_) => Err(Error::Parse(format!("`{field}` cannot be empty"))),
        None => Err(Error::Parse(format!(
            "`{field}` must be a string (got {})",
            json_type_name(value)
        ))),
    }
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Best-effort parse: coerce non-string fields and derive missing ones from
/// whatever is available. Returns `None` only when every content-bearing field
/// is empty or missing.
fn parse_submission_lenient(input: &serde_json::Value, usage: &Usage) -> Option<ParsedOutput> {
    let changelog = coerce_to_string(&input["changelog"]);
    let release_body = coerce_to_string(&input["release_body"]);
    let release_title = coerce_to_string(&input["release_title"]);

    let primary = release_body
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            changelog
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })?
        .to_string();

    let changelog = changelog
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| primary.clone());
    let release_body = release_body
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| primary.clone());
    let release_title = release_title
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| derive_title(&release_body));

    Some(ParsedOutput {
        changelog,
        release_title,
        release_body,
        usage: usage.clone(),
    })
}

fn coerce_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().filter_map(coerce_to_string).collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        serde_json::Value::Object(_) => serde_json::to_string_pretty(value).ok(),
    }
}

fn derive_title(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .map(|l| l.trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .unwrap_or("Release")
        .chars()
        .take(80)
        .collect()
}

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
    let mut malformed_submission_count = 0;
    let mut malformed_reasons: Vec<String> = Vec::new();
    let mut last_malformed_input: Option<serde_json::Value> = None;

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
        let mut malformed_submit = Vec::new();
        for tc in &response.tool_calls {
            if tc.name == "submit_release_notes" {
                match parse_submission(&tc.input, &total_usage) {
                    Ok(parsed) => submit = Some((tc.id.clone(), parsed)),
                    Err(Error::Parse(message)) => {
                        malformed_reasons.push(message.clone());
                        last_malformed_input = Some(tc.input.clone());
                        malformed_submit.push(ToolResult {
                            tool_call_id: tc.id.clone(),
                            content: format!(
                                "Error: {message}\n\nPlease call submit_release_notes again with non-empty string values for all three required fields: changelog, release_title, and release_body."
                            ),
                            is_error: true,
                        });
                    }
                    Err(err) => return Err(err),
                }
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

        if !malformed_submit.is_empty() {
            malformed_submission_count += malformed_submit.len();
            if malformed_submission_count >= MAX_MALFORMED_SUBMISSIONS {
                let received = last_malformed_input
                    .as_ref()
                    .and_then(|v| serde_json::to_string_pretty(v).ok())
                    .unwrap_or_else(|| "<no input captured>".into());
                if let Some(input) = &last_malformed_input
                    && let Some(parsed) = parse_submission_lenient(input, &total_usage)
                {
                    log::warn!(
                        "submit_release_notes was malformed {malformed_submission_count} times ({}); salvaging the last attempt. Received input:\n{received}",
                        malformed_reasons.join("; ")
                    );
                    return Ok(parsed);
                }
                return Err(Error::MalformedSubmission {
                    attempts: malformed_submission_count,
                    reasons: malformed_reasons.join("\n  - "),
                    src: miette::NamedSource::new("last submit_release_notes input", received),
                });
            }
            client.append_tool_results(&mut conversation, &malformed_submit);
            continue;
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

    #[test]
    fn test_field_as_string_rejects_empty() {
        let input = json!({ "changelog": "" });
        let err = field_as_string(&input, "changelog").unwrap_err();
        assert!(err.to_string().contains("cannot be empty"), "err: {err}");

        let input = json!({ "changelog": "   \n\t  " });
        let err = field_as_string(&input, "changelog").unwrap_err();
        assert!(err.to_string().contains("cannot be empty"), "err: {err}");
    }

    #[test]
    fn test_lenient_empty_release_body_falls_back_to_changelog() {
        // Regression: `coerce_to_string` returned Some("") for `[""]`, which
        // used to short-circuit the `.or(changelog)` fallback.
        let input = json!({
            "release_body": [""],
            "changelog": "- Fixed X",
        });
        let result = parse_submission_lenient(&input, &fake_usage()).unwrap();
        assert_eq!(result.changelog, "- Fixed X");
        assert_eq!(result.release_body, "- Fixed X");
    }

    #[test]
    fn test_derive_title_skips_empty_after_stripping_markers() {
        assert_eq!(derive_title("###\n\nReal title here"), "Real title here");
        assert_eq!(derive_title("#   \nActual content"), "Actual content");
        assert_eq!(derive_title(""), "Release");
        assert_eq!(derive_title("\n\n   \n"), "Release");
    }

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
    async fn test_missing_changelog_field_retries_submission() {
        let client = MockLlmClient::new(vec![
            TurnResponse {
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
            },
            TurnResponse {
                tool_calls: vec![submit_tool_call("log", "v1.0", "body")],
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
        assert_eq!(result.changelog, "log");
        assert_eq!(result.release_title, "v1.0");
        assert_eq!(result.release_body, "body");
    }

    #[tokio::test]
    async fn test_missing_release_title_field_retries_submission() {
        let client = MockLlmClient::new(vec![
            TurnResponse {
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
            },
            TurnResponse {
                tool_calls: vec![submit_tool_call("log", "v1.0", "body")],
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
        assert_eq!(result.changelog, "log");
        assert_eq!(result.release_title, "v1.0");
        assert_eq!(result.release_body, "body");
    }

    #[tokio::test]
    async fn test_missing_release_body_field_retries_submission() {
        let client = MockLlmClient::new(vec![
            TurnResponse {
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
            },
            TurnResponse {
                tool_calls: vec![submit_tool_call("log", "v1.0", "body")],
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
        assert_eq!(result.changelog, "log");
        assert_eq!(result.release_title, "v1.0");
        assert_eq!(result.release_body, "body");
    }

    #[tokio::test]
    async fn test_malformed_submission_salvages_partial() {
        // Model submits changelog + release_title but never release_body across
        // three attempts. Rather than failing, we salvage the partial submission
        // by deriving release_body from changelog.
        let partial = |id: &str| ToolCall {
            id: id.into(),
            name: "submit_release_notes".into(),
            input: json!({
                "changelog": "- Added X\n- Fixed Y",
                "release_title": "v1.0",
            }),
        };
        let client = MockLlmClient::new(vec![
            TurnResponse {
                tool_calls: vec![partial("call_1")],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            TurnResponse {
                tool_calls: vec![partial("call_2")],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            TurnResponse {
                tool_calls: vec![partial("call_3")],
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
        assert_eq!(result.changelog, "- Added X\n- Fixed Y");
        assert_eq!(result.release_title, "v1.0");
        assert_eq!(result.release_body, "- Added X\n- Fixed Y");
    }

    #[tokio::test]
    async fn test_malformed_submission_retry_limit_unsalvageable() {
        // All three content fields missing — nothing to salvage, so we error
        // out with a descriptive message listing the parse failures.
        let empty = |id: &str| ToolCall {
            id: id.into(),
            name: "submit_release_notes".into(),
            input: json!({}),
        };
        let client = MockLlmClient::new(vec![
            TurnResponse {
                tool_calls: vec![empty("call_1")],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            TurnResponse {
                tool_calls: vec![empty("call_2")],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            TurnResponse {
                tool_calls: vec![empty("call_3")],
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
        let err = run(ctx).await.unwrap_err();
        assert!(
            matches!(err, Error::MalformedSubmission { .. }),
            "err: {err:?}"
        );
        let msg = err.to_string();
        assert!(msg.contains("malformed 3 times"), "message: {msg}");
        // The specific reasons live in the miette help text, not the short
        // Display string. Confirm they're recorded on the variant directly.
        if let Error::MalformedSubmission { reasons, .. } = &err {
            assert!(reasons.contains("changelog"), "reasons: {reasons}");
        }
    }

    #[tokio::test]
    async fn test_lenient_coerces_non_string_fields() {
        // Model returns changelog as an array instead of a string. First two
        // attempts fail (retrying); on the third we salvage via coercion.
        let make = |id: &str| ToolCall {
            id: id.into(),
            name: "submit_release_notes".into(),
            input: json!({
                "changelog": ["- Added X", "- Fixed Y"],
                "release_title": "v2.0",
                "release_body": "Full body",
            }),
        };
        let client = MockLlmClient::new(vec![
            TurnResponse {
                tool_calls: vec![make("c1")],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            TurnResponse {
                tool_calls: vec![make("c2")],
                text: None,
                stop_reason: StopReason::ToolUse,
                usage: fake_usage(),
            },
            TurnResponse {
                tool_calls: vec![make("c3")],
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
        assert_eq!(result.changelog, "- Added X\n- Fixed Y");
        assert_eq!(result.release_title, "v2.0");
        assert_eq!(result.release_body, "Full body");
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
