use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::llm::{
    Conversation, LlmClient, StopReason, ToolCall, ToolDefinition, ToolResult, TurnResponse, Usage,
};

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: String,
    /// Cleared once an endpoint rejects `cache_control`, so Anthropic-compatible
    /// servers without prompt caching only pay for one failed request.
    prompt_caching: AtomicBool,
}

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    system: Value,
    messages: Vec<Value>,
    tools: Vec<ToolDef>,
}

/// Marks the end of a cacheable prefix. Anthropic caches everything up to and
/// including the block carrying it (tools, then system, then messages).
fn ephemeral() -> Value {
    json!({ "type": "ephemeral" })
}

#[derive(Debug, Clone, Serialize)]
struct ToolDef {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<Value>,
    stop_reason: Option<String>,
    usage: ApiUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ExtractedContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    input_tokens: u32,
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
}

impl AnthropicProvider {
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
            base_url,
            prompt_caching: AtomicBool::new(true),
        }
    }

    /// The agent loop resends the whole prefix every turn. With `cache`, mark
    /// the static tools + system prompt and the conversation so far as cache
    /// breakpoints so each turn only pays full price for what was appended.
    fn build_request(
        &self,
        system: &str,
        conversation: &Conversation,
        tools: &[ToolDefinition],
        cache: bool,
    ) -> MessagesRequest {
        let tools = tools
            .iter()
            .map(|t| ToolDef {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect();

        let mut messages = conversation.messages.clone();
        let system = if cache {
            if let Some(block) = messages
                .last_mut()
                .and_then(|m| m["content"].as_array_mut())
                .and_then(|content| content.last_mut())
                .and_then(Value::as_object_mut)
            {
                block.insert("cache_control".into(), ephemeral());
            }
            json!([{ "type": "text", "text": system, "cache_control": ephemeral() }])
        } else {
            json!(system)
        };

        MessagesRequest {
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            system,
            messages,
            tools,
        }
    }

    async fn post(&self, request: &MessagesRequest) -> Result<reqwest::Response> {
        crate::retry::retry_request("Anthropic API", || {
            self.client
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(request)
                .send()
        })
        .await
    }
}

impl LlmClient for AnthropicProvider {
    fn new_conversation(&self, user_message: &str) -> Conversation {
        let msg = json!({
            "role": "user",
            "content": [{ "type": "text", "text": user_message }]
        });
        Conversation {
            messages: vec![msg],
        }
    }

    fn append_tool_results(&self, conversation: &mut Conversation, results: &[ToolResult]) {
        let blocks: Vec<Value> = results
            .iter()
            .map(|r| {
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": r.tool_call_id,
                    "content": r.content,
                });
                if r.is_error {
                    block["is_error"] = json!(true);
                }
                block
            })
            .collect();
        conversation.messages.push(json!({
            "role": "user",
            "content": blocks,
        }));
    }

    fn send_turn<'a>(
        &'a self,
        system: &'a str,
        conversation: &'a mut Conversation,
        tools: &'a [ToolDefinition],
    ) -> Pin<Box<dyn Future<Output = Result<TurnResponse>> + Send + 'a>> {
        Box::pin(async move {
            let resp = if self.prompt_caching.load(Ordering::Relaxed) {
                let request = self.build_request(system, conversation, tools, true);
                let resp = self.post(&request).await?;
                if resp.status() == reqwest::StatusCode::BAD_REQUEST {
                    // Anthropic-compatible endpoints may reject `cache_control`
                    // or a block-array `system`. Retry once without caching and
                    // only stop caching if that succeeds, so an unrelated 400
                    // doesn't disable it.
                    let request = self.build_request(system, conversation, tools, false);
                    let retry = self.post(&request).await?;
                    if retry.status().is_success() {
                        log::warn!("endpoint rejected prompt caching; continuing without it");
                        self.prompt_caching.store(false, Ordering::Relaxed);
                        retry
                    } else {
                        resp
                    }
                } else {
                    resp
                }
            } else {
                let request = self.build_request(system, conversation, tools, false);
                self.post(&request).await?
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(Error::Llm(format!("{status}: {body}")));
            }

            let response: MessagesResponse = resp.json().await?;

            // Extract text and tool calls
            let mut text_parts = Vec::new();
            let mut tool_calls = Vec::new();
            for block in &response.content {
                match serde_json::from_value(block.clone())? {
                    ExtractedContentBlock::Text { text } => text_parts.push(text),
                    ExtractedContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(ToolCall { id, name, input });
                    }
                    ExtractedContentBlock::Other => {}
                }
            }

            // Preserve every assistant content block so Anthropic can verify
            // signed thinking blocks on the next turn in a tool-use loop.
            conversation.messages.push(json!({
                "role": "assistant",
                "content": response.content,
            }));

            let text = if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n"))
            };

            let stop_reason = match response.stop_reason.as_deref() {
                Some("tool_use") => StopReason::ToolUse,
                Some("end_turn") => StopReason::EndTurn,
                Some("max_tokens") => StopReason::MaxTokens,
                _ => StopReason::Unknown,
            };

            Ok(TurnResponse {
                tool_calls,
                text,
                stop_reason,
                usage: Usage {
                    input_tokens: response.usage.input_tokens,
                    output_tokens: response.usage.output_tokens,
                    cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
                    cache_read_input_tokens: response.usage.cache_read_input_tokens,
                },
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmClient, ToolResult};
    use serde_json::json;

    fn make_provider(base_url: &str) -> AnthropicProvider {
        AnthropicProvider::new("test-key".into(), "claude-3".into(), 1024, base_url.into())
    }

    #[test]
    fn test_new_conversation_format() {
        let provider = make_provider("http://localhost");
        let conv = provider.new_conversation("Hello");
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0]["role"], "user");
        assert_eq!(conv.messages[0]["content"][0]["type"], "text");
        assert_eq!(conv.messages[0]["content"][0]["text"], "Hello");
    }

    #[test]
    fn test_append_tool_results_format() {
        let provider = make_provider("http://localhost");
        let mut conv = provider.new_conversation("Hello");
        provider.append_tool_results(
            &mut conv,
            &[ToolResult {
                tool_call_id: "tc_1".into(),
                content: "result text".into(),
                is_error: false,
            }],
        );
        assert_eq!(conv.messages.len(), 2);
        let msg = &conv.messages[1];
        assert_eq!(msg["role"], "user");
        assert_eq!(msg["content"][0]["type"], "tool_result");
        assert_eq!(msg["content"][0]["tool_use_id"], "tc_1");
        assert_eq!(msg["content"][0]["content"], "result text");
        assert!(msg["content"][0].get("is_error").is_none());
    }

    #[test]
    fn test_append_tool_results_error_flag() {
        let provider = make_provider("http://localhost");
        let mut conv = provider.new_conversation("Hello");
        provider.append_tool_results(
            &mut conv,
            &[ToolResult {
                tool_call_id: "tc_1".into(),
                content: "error msg".into(),
                is_error: true,
            }],
        );
        let msg = &conv.messages[1];
        assert_eq!(msg["content"][0]["is_error"], true);
    }

    #[tokio::test]
    async fn test_send_turn_end_turn() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/messages"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "Hello!"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 10, "output_tokens": 5}
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Hi");
        let resp = provider.send_turn("system", &mut conv, &[]).await.unwrap();
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
        assert_eq!(conv.messages.len(), 2);
    }

    #[tokio::test]
    async fn test_send_turn_tool_use() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/messages"))
            .respond_with(
                wiremock::ResponseTemplate::new(200).set_body_json(json!({
                    "content": [
                        {"type": "thinking", "thinking": "", "signature": "signed-thinking"},
                        {"type": "text", "text": "Let me read that."},
                        {"type": "tool_use", "id": "tc_1", "name": "read_file", "input": {"path": "README.md"}}
                    ],
                    "stop_reason": "tool_use",
                    "usage": {"input_tokens": 20, "output_tokens": 15}
                })),
            )
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Read the readme");
        let resp = provider.send_turn("system", &mut conv, &[]).await.unwrap();
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "read_file");
        assert_eq!(resp.tool_calls[0].input["path"], "README.md");
        assert_eq!(conv.messages[1]["content"][0]["type"], "thinking");
        assert_eq!(
            conv.messages[1]["content"][0]["signature"],
            "signed-thinking"
        );

        provider.append_tool_results(
            &mut conv,
            &[ToolResult {
                tool_call_id: "tc_1".into(),
                content: "contents".into(),
                is_error: false,
            }],
        );
        provider.send_turn("system", &mut conv, &[]).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let second_request: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(
            second_request["messages"][1]["content"][0],
            json!({"type": "thinking", "thinking": "", "signature": "signed-thinking"})
        );
    }

    #[tokio::test]
    async fn test_send_turn_marks_prompt_cache_breakpoints() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/messages"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 3,
                    "output_tokens": 5,
                    "cache_creation_input_tokens": 100,
                    "cache_read_input_tokens": 400
                }
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Hi");
        let resp = provider
            .send_turn("system prompt", &mut conv, &[])
            .await
            .unwrap();
        assert_eq!(resp.usage.input_tokens, 3);
        assert_eq!(resp.usage.cache_creation_input_tokens, 100);
        assert_eq!(resp.usage.cache_read_input_tokens, 400);
        assert_eq!(resp.usage.total_input_tokens(), 503);

        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(
            body["system"],
            json!([{
                "type": "text",
                "text": "system prompt",
                "cache_control": {"type": "ephemeral"}
            }])
        );
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        // The stored conversation must stay marker-free so breakpoints move
        // forward with each turn instead of accumulating past the 4 allowed.
        assert!(
            conv.messages[0]["content"][0]
                .get("cache_control")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_send_turn_moves_cache_breakpoint_to_latest_message() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/messages"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "content": [
                    {"type": "tool_use", "id": "tc_1", "name": "read_file", "input": {}}
                ],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 1, "output_tokens": 1}
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Hi");
        provider.send_turn("s", &mut conv, &[]).await.unwrap();
        provider.append_tool_results(
            &mut conv,
            &[ToolResult {
                tool_call_id: "tc_1".into(),
                content: "a".into(),
                is_error: false,
            }],
        );
        provider.send_turn("s", &mut conv, &[]).await.unwrap();

        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
        let marked = body.to_string().matches("cache_control").count();
        // One on system, one on the last message (the tool result).
        assert_eq!(marked, 2);
        assert_eq!(
            body["messages"][2]["content"][0]["cache_control"],
            json!({"type": "ephemeral"})
        );
        assert!(
            body["messages"][0]["content"][0]
                .get("cache_control")
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_send_turn_falls_back_when_caching_is_rejected() {
        let server = wiremock::MockServer::start().await;
        // Endpoint that rejects any request carrying cache_control.
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::body_string_contains("cache_control"))
            .respond_with(wiremock::ResponseTemplate::new(400).set_body_string("no caching"))
            .mount(&server)
            .await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1}
            })))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Hi");
        let resp = provider.send_turn("sys", &mut conv, &[]).await.unwrap();
        assert_eq!(resp.text.as_deref(), Some("ok"));

        // Later turns skip caching entirely rather than failing first again.
        provider.send_turn("sys", &mut conv, &[]).await.unwrap();
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 3);
        let body: Value = serde_json::from_slice(&requests[1].body).unwrap();
        assert_eq!(body["system"], json!("sys"));
        assert!(!requests[2].body.windows(13).any(|w| w == b"cache_control"));
    }

    #[tokio::test]
    async fn test_send_turn_api_error() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/messages"))
            .respond_with(wiremock::ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Hi");
        let err = provider
            .send_turn("system", &mut conv, &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("401"));
    }
}
