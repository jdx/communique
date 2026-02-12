use std::future::Future;
use std::pin::Pin;

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
            base_url,
        }
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
            conversation.messages.push(json!({
                "role": "tool",
                "tool_call_id": r.tool_call_id,
                "content": r.content,
            }));
        }
    }

    fn send_turn<'a>(
        &'a self,
        system: &'a str,
        conversation: &'a mut Conversation,
        tools: &'a [ToolDefinition],
    ) -> Pin<Box<dyn Future<Output = Result<TurnResponse>> + Send + 'a>> {
        Box::pin(async move {
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
                "max_tokens": self.max_tokens,
                "messages": messages,
            });
            if !tool_defs.is_empty() {
                body["tools"] = json!(tool_defs);
            }

            let mut req = self
                .client
                .post(format!("{}/chat/completions", self.base_url))
                .json(&body);

            if !self.api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", self.api_key));
            }

            let resp = req.send().await?;

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
                },
                None => Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
            };

            Ok(TurnResponse {
                tool_calls,
                stop_reason,
                usage,
            })
        })
    }
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
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": {"content": "Hello!", "tool_calls": null},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 10, "completion_tokens": 5}
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
            .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("internal error"))
            .mount(&server)
            .await;

        let provider = make_provider(&server.uri());
        let mut conv = provider.new_conversation("Hi");
        let err = provider
            .send_turn("system", &mut conv, &[])
            .await
            .unwrap_err();
        assert!(err.to_string().contains("500"));
    }
}
