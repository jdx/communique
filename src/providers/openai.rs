use std::future::Future;
use std::pin::Pin;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::llm::{
    Conversation, LlmClient, StopReason, ToolCall, ToolDefinition, ToolResult, TurnResponse, Usage,
};

pub struct OpenAIProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: String,
    headers: HeaderMap,
    use_responses: bool,
}

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output: Vec<Value>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    incomplete_details: Option<IncompleteDetails>,
    usage: Option<ResponsesUsage>,
}

#[derive(Debug, Deserialize)]
struct IncompleteDetails {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    input_tokens_details: Option<InputTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct InputTokensDetails {
    cached_tokens: u32,
    #[serde(default)]
    cache_write_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<ApiUsage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ApiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ApiToolCall {
    id: String,
    function: ApiFunction,
}

#[derive(Debug, Deserialize)]
struct ApiFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

impl OpenAIProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32, base_url: String) -> Self {
        let client = reqwest::Client::builder()
            .user_agent("communique/0.1")
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            api_key,
            model,
            max_tokens,
            // Current OpenAI reasoning models reject function tools on
            // /chat/completions, so api.openai.com uses /responses. Custom
            // endpoints generally only implement chat completions.
            use_responses: base_url.trim_end_matches('/') == DEFAULT_BASE_URL,
            base_url,
            headers: HeaderMap::new(),
        }
    }

    #[cfg(test)]
    /// Override which endpoint is used: `true` for `/responses`, `false` for
    /// `/chat/completions`.
    pub fn with_responses_api(mut self, use_responses: bool) -> Self {
        self.use_responses = use_responses;
        self
    }

    async fn send_responses(
        &self,
        system: &str,
        conversation: &mut Conversation,
        tools: &[ToolDefinition],
        custom_headers: HeaderMap,
    ) -> Result<TurnResponse> {
        let tool_defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                    // Responses defaults to strict mode, which would make every
                    // optional argument required.
                    "strict": false,
                })
            })
            .collect();

        let mut body = json!({
            "model": self.model,
            "instructions": system,
            "input": conversation.messages,
            "max_output_tokens": self.max_tokens,
            "store": false,
        });
        // Encrypted reasoning is only available on reasoning models; others
        // reject the `include` parameter.
        if uses_max_completion_tokens(&self.model) {
            body["include"] = json!(["reasoning.encrypted_content"]);
        }
        if !tool_defs.is_empty() {
            body["tools"] = json!(tool_defs);
        }

        let resp = crate::retry::retry_request("OpenAI API", || {
            let mut req = self
                .client
                .post(format!("{}/responses", self.base_url))
                .json(&body);
            if !self.api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", self.api_key));
            }
            req.headers(custom_headers.clone()).send()
        })
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Llm(format!("{status}: {body}")));
        }

        let response: ResponsesResponse = resp.json().await?;

        let mut tool_calls = vec![];
        let mut truncated_call = false;
        let mut text = String::new();
        for item in &response.output {
            match item["type"].as_str() {
                Some("function_call") => {
                    if item["status"] == "incomplete" {
                        // Arguments were cut off; never dispatch a partial call.
                        truncated_call = true;
                        continue;
                    }
                    let name = item["name"].as_str().unwrap_or_default();
                    let arguments = item["arguments"].as_str().unwrap_or("{}");
                    let input = serde_json::from_str(arguments).map_err(|_| {
                        Error::Llm(format!("model sent invalid JSON arguments for tool {name}"))
                    })?;
                    tool_calls.push(ToolCall {
                        id: item["call_id"].as_str().unwrap_or_default().to_string(),
                        name: name.to_string(),
                        input,
                    });
                }
                Some("message") => {
                    for part in item["content"].as_array().into_iter().flatten() {
                        if part["type"] == "output_text"
                            && let Some(t) = part["text"].as_str()
                        {
                            text.push_str(t);
                        }
                    }
                }
                _ => {}
            }
        }

        // Output items (including reasoning) are replayed as input next turn.
        conversation.messages.extend(response.output);

        let incomplete_reason = response
            .incomplete_details
            .as_ref()
            .and_then(|d| d.reason.as_deref());
        if incomplete_reason == Some("content_filter") {
            // Never treat filtered, partial text as finished release notes.
            return Err(Error::Llm(
                "OpenAI stopped the response because of its content filter".into(),
            ));
        }
        let hit_token_limit = response
            .incomplete_details
            .as_ref()
            .and_then(|d| d.reason.as_deref())
            == Some("max_output_tokens");
        let stop_reason = if truncated_call {
            tool_calls.clear();
            if hit_token_limit {
                StopReason::MaxTokens
            } else {
                StopReason::Unknown
            }
        } else if !tool_calls.is_empty() {
            StopReason::ToolUse
        } else {
            match response.status.as_deref() {
                Some("completed") => StopReason::EndTurn,
                Some("incomplete") if hit_token_limit => StopReason::MaxTokens,
                _ => StopReason::Unknown,
            }
        };

        // OpenAI counts cache reads inside `input_tokens`.
        let usage = response.usage.map_or_else(Usage::default, |u| {
            let (read, written) = u
                .input_tokens_details
                .map_or((0, 0), |d| (d.cached_tokens, d.cache_write_tokens));
            let read = read.min(u.input_tokens);
            let written = written.min(u.input_tokens - read);
            Usage {
                input_tokens: u.input_tokens - read - written,
                output_tokens: u.output_tokens,
                cache_creation_input_tokens: written,
                cache_read_input_tokens: read,
            }
        });

        Ok(TurnResponse {
            tool_calls,
            text: (!text.is_empty()).then_some(text),
            stop_reason,
            usage,
        })
    }

    pub fn with_headers(mut self, headers: &[String]) -> Result<Self> {
        for header in headers {
            let (name, value) = header
                .split_once(':')
                .ok_or_else(|| Error::Llm("--header must use name: value format".into()))?;
            let name = HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|_| Error::Llm("--header contains an invalid header name".into()))?;
            let mut value = HeaderValue::from_str(value.trim())
                .map_err(|_| Error::Llm("--header contains an invalid header value".into()))?;
            value.set_sensitive(true);
            self.headers.insert(name, value);
        }
        Ok(self)
    }
}

impl LlmClient for OpenAIProvider {
    fn new_conversation(&self, user_message: &str) -> Conversation {
        let msg = json!({
            "role": "user",
            "content": user_message,
        });
        Conversation {
            messages: vec![msg],
        }
    }

    fn append_tool_results(&self, conversation: &mut Conversation, results: &[ToolResult]) {
        for r in results {
            conversation.messages.push(if self.use_responses {
                json!({
                    "type": "function_call_output",
                    "call_id": r.tool_call_id,
                    "output": r.content,
                })
            } else {
                json!({
                    "role": "tool",
                    "tool_call_id": r.tool_call_id,
                    "content": r.content,
                })
            });
        }
    }

    fn send_turn<'a>(
        &'a self,
        system: &'a str,
        conversation: &'a mut Conversation,
        tools: &'a [ToolDefinition],
    ) -> Pin<Box<dyn Future<Output = Result<TurnResponse>> + Send + 'a>> {
        Box::pin(async move {
            let mut custom_headers = match std::env::var("OPENAI_CUSTOM_HEADERS") {
                Ok(value) => parse_custom_headers(&value)?,
                Err(std::env::VarError::NotPresent) => HeaderMap::new(),
                Err(_) => {
                    return Err(Error::Llm(
                        "OPENAI_CUSTOM_HEADERS must contain valid Unicode".into(),
                    ));
                }
            };

            custom_headers.extend(self.headers.clone());

            if self.use_responses {
                return self
                    .send_responses(system, conversation, tools, custom_headers)
                    .await;
            }

            // Build messages array: system message + conversation messages
            let mut messages = vec![json!({
                "role": "system",
                "content": system,
            })];
            messages.extend(conversation.messages.iter().cloned());

            let tool_defs: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        }
                    })
                })
                .collect();

            let mut body = json!({
                "model": self.model,
                "messages": messages,
            });
            if uses_max_completion_tokens(&self.model) {
                body["max_completion_tokens"] = json!(self.max_tokens);
            } else {
                body["max_tokens"] = json!(self.max_tokens);
            }
            if !tool_defs.is_empty() {
                body["tools"] = json!(tool_defs);
            }

            let resp = crate::retry::retry_request("OpenAI API", || {
                let mut req = self
                    .client
                    .post(format!("{}/chat/completions", self.base_url))
                    .json(&body);
                if !self.api_key.is_empty() {
                    req = req.header("Authorization", format!("Bearer {}", self.api_key));
                }
                req.headers(custom_headers.clone()).send()
            })
            .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(Error::Llm(format!("{status}: {body}")));
            }

            let response: ChatResponse = resp.json().await?;

            let choice = response
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| Error::Llm("no choices in response".into()))?;

            // Build the assistant message for conversation history
            let mut assistant_msg = json!({ "role": "assistant" });
            if let Some(content) = &choice.message.content {
                assistant_msg["content"] = json!(content);
            }
            if let Some(ref tc) = choice.message.tool_calls {
                let calls: Vec<Value> = tc
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": c.function.name,
                                "arguments": c.function.arguments,
                            }
                        })
                    })
                    .collect();
                assistant_msg["tool_calls"] = json!(calls);
            }
            conversation.messages.push(assistant_msg);

            // Extract tool calls
            let tool_calls = match choice.message.tool_calls {
                Some(calls) => calls
                    .into_iter()
                    .map(|c| {
                        let input: Value =
                            serde_json::from_str(&c.function.arguments).unwrap_or(json!({}));
                        ToolCall {
                            id: c.id,
                            name: c.function.name,
                            input,
                        }
                    })
                    .collect(),
                None => vec![],
            };

            let stop_reason = match choice.finish_reason.as_deref() {
                Some("tool_calls") => StopReason::ToolUse,
                Some("stop") => StopReason::EndTurn,
                Some("length") => StopReason::MaxTokens,
                _ => StopReason::Unknown,
            };

            let usage = match response.usage {
                Some(u) => Usage {
                    input_tokens: u.prompt_tokens,
                    output_tokens: u.completion_tokens,
                    ..Usage::default()
                },
                None => Usage::default(),
            };

            Ok(TurnResponse {
                tool_calls,
                text: choice.message.content,
                stop_reason,
                usage,
            })
        })
    }
}

/// Parse without including header values (which may be secrets) in diagnostics.
fn parse_custom_headers(value: &str) -> Result<HeaderMap> {
    if value.trim().is_empty() {
        return Ok(HeaderMap::new());
    }
    let entries: std::collections::BTreeMap<String, String> =
        serde_json::from_str(value).map_err(|_| {
            Error::Llm("OPENAI_CUSTOM_HEADERS must be a JSON object with string values".into())
        })?;
    let mut headers = HeaderMap::new();
    for (name, value) in entries {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            Error::Llm("OPENAI_CUSTOM_HEADERS contains an invalid header name".into())
        })?;
        let mut value = HeaderValue::from_str(&value).map_err(|_| {
            Error::Llm("OPENAI_CUSTOM_HEADERS contains an invalid header value".into())
        })?;
        value.set_sensitive(true);
        headers.insert(name, value);
    }
    Ok(headers)
}

fn uses_max_completion_tokens(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    let model_name = model.rsplit([':', '/']).next().unwrap();
    gpt_model_number(model_name).is_some_and(|number| number >= 5)
        || model_name
            .strip_prefix('o')
            .is_some_and(|suffix| suffix.starts_with(|c: char| c.is_ascii_digit()))
}

fn gpt_model_number(model: &str) -> Option<u32> {
    let segment = model.strip_prefix("gpt-")?.split(['.', '-']).next()?;
    let numeric = segment.trim_end_matches(|c: char| !c.is_ascii_digit());
    numeric.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmClient, ToolResult};
    use serde_json::json;

    fn make_provider(base_url: &str) -> OpenAIProvider {
        OpenAIProvider::new("test-key".into(), "gpt-4".into(), 1024, base_url.into())
    }

    #[test]
    fn test_invalid_cli_headers() {
        for invalid in ["missing-colon", "bad name: secret", "x-test: secret\nvalue"] {
            let error = make_provider("http://localhost")
                .with_headers(&[invalid.into()])
                .err()
                .expect("invalid header must fail")
                .to_string();
            assert!(error.contains("--header"));
            assert!(!error.contains("secret"));
        }
    }

    #[test]
    fn test_custom_headers() {
        let headers = parse_custom_headers(
            r#"{"copilot-integration-id":"copilot-developer-cli","X-Custom":"value"}"#,
        )
        .unwrap();
        assert_eq!(headers["copilot-integration-id"], "copilot-developer-cli");
        assert_eq!(headers["x-custom"], "value");
        assert!(parse_custom_headers("").unwrap().is_empty());
        for invalid in [
            "not json",
            "[]",
            r#"{"x":1}"#,
            r#"{"bad name":"secret"}"#,
            r#"{"x":"secret\nvalue"}"#,
        ] {
            let error = parse_custom_headers(invalid).unwrap_err().to_string();
            assert!(error.contains("OPENAI_CUSTOM_HEADERS"));
            assert!(!error.contains("secret"));
        }
    }

    #[test]
    fn test_new_conversation_format() {
        let provider = make_provider("http://localhost");
        let conv = provider.new_conversation("Hello");
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0]["role"], "user");
        assert_eq!(conv.messages[0]["content"], "Hello");
    }

    #[test]
    fn test_append_tool_results_format() {
        let provider = make_provider("http://localhost");
        let mut conv = provider.new_conversation("Hello");
        provider.append_tool_results(
            &mut conv,
            &[
                ToolResult {
                    tool_call_id: "tc_1".into(),
                    content: "result 1".into(),
                    is_error: false,
                },
                ToolResult {
                    tool_call_id: "tc_2".into(),
                    content: "result 2".into(),
                    is_error: false,
                },
            ],
        );
        // OpenAI adds one message per tool result
        assert_eq!(conv.messages.len(), 3);
        assert_eq!(conv.messages[1]["role"], "tool");
        assert_eq!(conv.messages[1]["tool_call_id"], "tc_1");
        assert_eq!(conv.messages[2]["role"], "tool");
        assert_eq!(conv.messages[2]["tool_call_id"], "tc_2");
    }

    #[tokio::test]
    async fn test_send_turn_end_turn() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/chat/completions"))
            .and(wiremock::matchers::header(
                "copilot-integration-id",
                "copilot-developer-cli",
            ))
            .and(wiremock::matchers::header("authorization", "custom-token"))
            .and(wiremock::matchers::body_partial_json(json!({
                "model": "gpt-4",
                "max_tokens": 1024,
            })))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {"content": "Hello!", "tool_calls": null},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5}
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri())
            .with_headers(&[
                "copilot-integration-id: old".into(),
                "Copilot-Integration-Id: copilot-developer-cli".into(),
                "Authorization: custom-token".into(),
            ])
            .unwrap();
        let mut conv = provider.new_conversation("Hi");
        let resp = provider.send_turn("system", &mut conv, &[]).await.unwrap();
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[tokio::test]
    async fn test_send_turn_uses_max_completion_tokens_for_gpt5() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/chat/completions"))
            .and(wiremock::matchers::body_partial_json(json!({
                "model": "gpt-5.1",
                "max_completion_tokens": 2048,
            })))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {"content": "Hello!", "tool_calls": null},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5}
            })))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new("test-key".into(), "gpt-5.1".into(), 2048, server.uri());
        let mut conv = provider.new_conversation("Hi");
        let resp = provider.send_turn("system", &mut conv, &[]).await.unwrap();
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn test_uses_max_completion_tokens_model_detection() {
        assert!(uses_max_completion_tokens("gpt-5.1"));
        assert!(uses_max_completion_tokens("gpt-5"));
        assert!(uses_max_completion_tokens("gpt-5o"));
        assert!(uses_max_completion_tokens("gpt-6"));
        assert!(uses_max_completion_tokens("openai/gpt-5.1"));
        assert!(uses_max_completion_tokens("o1-mini"));
        assert!(uses_max_completion_tokens("o1-preview"));
        assert!(uses_max_completion_tokens("o3-mini"));
        assert!(uses_max_completion_tokens("openai/o4-mini"));
        assert!(uses_max_completion_tokens("o5"));
        assert!(!uses_max_completion_tokens("gpt-4"));
        assert!(!uses_max_completion_tokens("gpt-4o"));
        assert!(!uses_max_completion_tokens("openrouter"));
        assert!(!uses_max_completion_tokens("test-model"));
    }

    #[tokio::test]
    async fn test_send_turn_tool_calls() {
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
                                "name": "read_file",
                                "arguments": "{\"path\":\"README.md\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 20, "completion_tokens": 10}
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Read the readme");
        let resp = provider.send_turn("system", &mut conv, &[]).await.unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "read_file");
        assert_eq!(resp.tool_calls[0].input["path"], "README.md");
    }

    #[tokio::test]
    async fn test_send_turn_api_error() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/chat/completions"))
            .respond_with(wiremock::ResponseTemplate::new(400).set_body_string("bad request"))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Hi");
        let err = provider
            .send_turn("system", &mut conv, &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("400"));
    }

    #[test]
    fn test_responses_api_only_for_default_base_url() {
        let new = |url: &str| OpenAIProvider::new("k".into(), "m".into(), 1, url.into());
        assert!(new("https://api.openai.com/v1").use_responses);
        assert!(new("https://api.openai.com/v1/").use_responses);
        assert!(!new("http://localhost:11434/v1").use_responses);
        assert!(
            !new("http://localhost")
                .with_responses_api(false)
                .use_responses
        );
        assert!(
            new("http://localhost")
                .with_responses_api(true)
                .use_responses
        );
    }

    #[tokio::test]
    async fn test_responses_tool_round_trip() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/responses"))
            .and(wiremock::matchers::body_partial_json(json!({
                "instructions": "system",
                "store": false,
                "max_output_tokens": 1024,
                "tools": [{"type": "function", "name": "read_file"}],
            })))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "status": "completed",
                "output": [
                    {"type": "reasoning", "id": "rs_1", "summary": [], "encrypted_content": "abc"},
                    {
                        "type": "function_call",
                        "id": "fc_1",
                        "call_id": "call_1",
                        "name": "read_file",
                        "arguments": "{\"path\":\"README.md\"}"
                    }
                ],
                "usage": {
                    "input_tokens": 20,
                    "output_tokens": 10,
                    "input_tokens_details": {"cached_tokens": 3, "cache_write_tokens": 5}
                }
            })))
            .expect(2)
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new("test-key".into(), "gpt-5".into(), 1024, server.uri())
            .with_responses_api(true);
        let mut conv = provider.new_conversation("Read the readme");
        let tools = [ToolDefinition {
            name: "read_file".into(),
            description: "Read".into(),
            input_schema: json!({"type": "object"}),
        }];
        let resp = provider
            .send_turn("system", &mut conv, &tools)
            .await
            .unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.tool_calls[0].id, "call_1");
        assert_eq!(resp.tool_calls[0].input["path"], "README.md");
        assert_eq!(resp.usage.input_tokens, 12);
        assert_eq!(resp.usage.cache_read_input_tokens, 3);
        assert_eq!(resp.usage.cache_creation_input_tokens, 5);
        assert_eq!(resp.usage.output_tokens, 10);

        provider.append_tool_results(
            &mut conv,
            &[ToolResult {
                tool_call_id: "call_1".into(),
                content: "contents".into(),
                is_error: false,
            }],
        );
        // user, reasoning, function_call, function_call_output
        assert_eq!(conv.messages.len(), 4);
        assert_eq!(conv.messages[1]["type"], "reasoning");
        assert_eq!(conv.messages[3]["type"], "function_call_output");
        assert_eq!(conv.messages[3]["call_id"], "call_1");
        assert_eq!(conv.messages[3]["output"], "contents");

        provider
            .send_turn("system", &mut conv, &tools)
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let second: Value = requests[1].body_json().unwrap();
        assert_eq!(second["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(second["tools"][0]["strict"], false);
        let input = second["input"].as_array().unwrap();
        assert_eq!(input[1]["type"], "reasoning");
        assert_eq!(input[1]["encrypted_content"], "abc");
        assert_eq!(input[2]["call_id"], "call_1");
        assert_eq!(input[3]["type"], "function_call_output");
        assert_eq!(input[3]["call_id"], "call_1");
    }

    #[tokio::test]
    async fn test_responses_incomplete_call_not_dispatched() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/responses"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "status": "incomplete",
                "incomplete_details": {"reason": "max_output_tokens"},
                "output": [{
                    "type": "function_call",
                    "status": "incomplete",
                    "call_id": "call_1",
                    "name": "get_commits",
                    "arguments": "{\"fro"
                }],
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri()).with_responses_api(true);
        let mut conv = provider.new_conversation("Hi");
        let resp = provider.send_turn("system", &mut conv, &[]).await.unwrap();
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.stop_reason, StopReason::MaxTokens);
    }

    #[tokio::test]
    async fn test_responses_content_filter_is_an_error() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/responses"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "status": "incomplete",
                "incomplete_details": {"reason": "content_filter"},
                "output": [],
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri()).with_responses_api(true);
        let mut conv = provider.new_conversation("Hi");
        let err = provider
            .send_turn("system", &mut conv, &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("content filter"));
    }

    #[tokio::test]
    async fn test_responses_invalid_arguments_rejected() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/responses"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "status": "completed",
                "output": [{
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "get_commits",
                    "arguments": "{not json"
                }],
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri()).with_responses_api(true);
        let mut conv = provider.new_conversation("Hi");
        let err = provider
            .send_turn("system", &mut conv, &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("invalid JSON"));
    }

    #[tokio::test]
    async fn test_responses_text_and_incomplete() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/responses"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "status": "incomplete",
                "incomplete_details": {"reason": "max_output_tokens"},
                "output": [{
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": "partial"}]
                }],
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri()).with_responses_api(true);
        let mut conv = provider.new_conversation("Hi");
        let resp = provider.send_turn("system", &mut conv, &[]).await.unwrap();
        assert_eq!(resp.stop_reason, StopReason::MaxTokens);
        assert_eq!(resp.text.as_deref(), Some("partial"));
        assert!(resp.tool_calls.is_empty());
    }
}
