pub mod anthropic;
pub mod openai;

use crate::llm::LlmClient;

#[derive(Debug, Clone, PartialEq)]
pub enum Provider {
    Anthropic,
    OpenAI,
}

/// Auto-detect provider from model name: `claude*` → Anthropic, everything else → OpenAI.
pub fn detect_provider(model: &str) -> Provider {
    if model.starts_with("claude") {
        Provider::Anthropic
    } else {
        Provider::OpenAI
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_provider_claude() {
        assert_eq!(detect_provider("claude-opus-4-6"), Provider::Anthropic);
        assert_eq!(
            detect_provider("claude-sonnet-4-5-20250929"),
            Provider::Anthropic
        );
    }

    #[test]
    fn test_detect_provider_non_claude() {
        assert_eq!(detect_provider("gpt-4"), Provider::OpenAI);
        assert_eq!(detect_provider("llama-3"), Provider::OpenAI);
    }
}

pub fn build_client(
    provider: &Provider,
    api_key: String,
    model: String,
    max_tokens: u32,
    base_url: Option<String>,
) -> Box<dyn LlmClient> {
    match provider {
        Provider::Anthropic => {
            let base = base_url.unwrap_or_else(|| "https://api.anthropic.com".into());
            Box::new(anthropic::AnthropicProvider::new(
                api_key, model, max_tokens, base,
            ))
        }
        Provider::OpenAI => {
            let base = base_url.unwrap_or_else(|| "https://api.openai.com/v1".into());
            Box::new(openai::OpenAIProvider::new(
                api_key, model, max_tokens, base,
            ))
        }
    }
}
