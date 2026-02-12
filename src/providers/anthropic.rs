use std::future::Future;
use std::pin::Pin;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
    tools: Vec<ToolDef>,
}

#[derive(Debug, Clone, Serialize)]
struct ToolDef {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    stop_reason: Option<String>,
    usage: ApiUsage,
}

#[derive(Debug, Deserialize)]
struct ApiUsage {
    input_tokens: u32,
    output_tokens: u32,
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
        }
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
            let messages: Vec<Message> = conversation
                .messages
                .iter()
                .map(|v| serde_json::from_value(v.clone()).expect("invalid conversation message"))
                .collect();

            let tool_defs: Vec<ToolDef> = tools
                .iter()
                .map(|t| ToolDef {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: t.input_schema.clone(),
                })
                .collect();

            let request = MessagesRequest {
                model: self.model.clone(),
                max_tokens: self.max_tokens,
                system: system.into(),
                messages,
                tools: tool_defs,
            };

            let resp = self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&request)
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(Error::Llm(format!("{status}: {body}")));
            }

            let response: MessagesResponse = resp.json().await?;

            // Append assistant message to conversation
            let assistant_content: Vec<Value> = response
                .content
                .iter()
                .map(|b| serde_json::to_value(b).unwrap())
                .collect();
            conversation.messages.push(json!({
                "role": "assistant",
                "content": assistant_content,
            }));

            // Extract tool calls
            let tool_calls: Vec<ToolCall> = response
                .content
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                    }),
                    _ => None,
                })
                .collect();

            let stop_reason = match response.stop_reason.as_deref() {
                Some("tool_use") => StopReason::ToolUse,
                Some("end_turn") => StopReason::EndTurn,
                Some("max_tokens") => StopReason::MaxTokens,
                _ => StopReason::Unknown,
            };

            Ok(TurnResponse {
                tool_calls,
                stop_reason,
                usage: Usage {
                    input_tokens: response.usage.input_tokens,
                    output_tokens: response.usage.output_tokens,
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
    }

    #[tokio::test]
    async fn test_send_turn_api_error() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/v1/messages"))
            .respond_with(wiremock::ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Hi");
        let err = provider
            .send_turn("system", &mut conv, &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("429"));
    }
}
