use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::error::Result;

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Opaque conversation state — each provider stores messages in its native format.
#[derive(Debug)]
pub struct Conversation {
    pub messages: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    ToolUse,
    EndTurn,
    MaxTokens,
    Unknown,
}

#[derive(Debug)]
pub struct TurnResponse {
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

pub trait LlmClient: Send + Sync {
    fn new_conversation(&self, user_message: &str) -> Conversation;
    fn append_tool_results(&self, conversation: &mut Conversation, results: &[ToolResult]);
    fn send_turn<'a>(
        &'a self,
        system: &'a str,
        conversation: &'a mut Conversation,
        tools: &'a [ToolDefinition],
    ) -> Pin<Box<dyn Future<Output = Result<TurnResponse>> + Send + 'a>>;
}
