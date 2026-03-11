use serde::{Deserialize, Serialize};

use super::{ChatMessage, LlmProvider, LlmProviderConfig, TokenUsage};

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    max_tokens: usize,
    response_format: ResponseFormat,
}

#[derive(Serialize)]
struct ResponseFormat {
    r#type: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

/// OpenAI-compatible LLM provider.
pub struct OpenAiProvider {
    config: LlmProviderConfig,
}

impl OpenAiProvider {
    pub fn new(config: LlmProviderConfig) -> Self {
        Self { config }
    }
}

impl LlmProvider for OpenAiProvider {
    fn config(&self) -> &LlmProviderConfig {
        &self.config
    }

    fn build_request_body(
        &self,
        system_prompt: &str,
        messages: &[ChatMessage],
        max_tokens: usize,
    ) -> Result<String, String> {
        let mut all_messages = vec![ChatMessage {
            role: "system".into(),
            content: system_prompt.into(),
        }];
        all_messages.extend_from_slice(messages);

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: all_messages,
            max_tokens,
            response_format: ResponseFormat {
                r#type: "json_object".into(),
            },
        };

        serde_json::to_string(&request).map_err(|e| e.to_string())
    }

    fn endpoint_path(&self) -> &str {
        "/chat/completions"
    }

    fn auth_headers(&self) -> Vec<String> {
        vec![format!("Authorization: Bearer {}", self.config.api_key)]
    }

    fn parse_response(&self, body: &str) -> Result<(String, Option<TokenUsage>), String> {
        let resp: ChatResponse = serde_json::from_str(body)
            .map_err(|e| format!("failed to parse LLM response: {e}\nbody: {body}"))?;

        let usage = resp.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        let content = resp
            .choices
            .into_iter()
            .next()
            .ok_or("no choices in LLM response")?
            .message
            .content;

        Ok((content, usage))
    }
}
