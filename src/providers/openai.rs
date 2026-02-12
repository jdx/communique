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
