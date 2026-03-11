use serde::{Deserialize, Serialize};

use super::{ChatMessage, LlmProvider, LlmProviderConfig, TokenUsage};

#[derive(Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: usize,
    system: Vec<SystemBlock>,
    messages: Vec<AnthropicMessage>,
}

#[derive(Serialize)]
struct SystemBlock {
    r#type: String,
    text: String,
    cache_control: CacheControl,
}

#[derive(Serialize)]
struct CacheControl {
    r#type: String,
}

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: String,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    input_tokens: usize,
    output_tokens: usize,
    #[serde(default)]
    cache_creation_input_tokens: usize,
    #[serde(default)]
    cache_read_input_tokens: usize,
}

/// Anthropic native API provider.
pub struct AnthropicProvider {
    config: LlmProviderConfig,
}

impl AnthropicProvider {
    pub fn new(config: LlmProviderConfig) -> Self {
        Self { config }
    }
}

impl LlmProvider for AnthropicProvider {
    fn config(&self) -> &LlmProviderConfig {
        &self.config
    }

    fn build_request_body(
        &self,
        system_prompt: &str,
        messages: &[ChatMessage],
        max_tokens: usize,
    ) -> Result<String, String> {
        // Filter out system messages — Anthropic uses top-level `system` field.
        // Map remaining messages, converting "system" role retry hints to "user".
        let api_messages: Vec<AnthropicMessage> = messages
            .iter()
            .filter(|m| m.role != "system" || !m.content.contains("Error from previous attempt"))
            .map(|m| {
                let role = if m.role == "system" {
                    "user".to_string()
                } else {
                    m.role.clone()
                };
                AnthropicMessage {
                    role,
                    content: m.content.clone(),
                }
            })
            .collect();

        // Deduplicate consecutive same-role messages (Anthropic requires alternation)
        let mut deduped: Vec<AnthropicMessage> = Vec::new();
        for msg in api_messages {
            if let Some(last) = deduped.last_mut() {
                if last.role == msg.role {
                    last.content.push_str("\n\n");
                    last.content.push_str(&msg.content);
                    continue;
                }
            }
            deduped.push(msg);
        }

        let system_with_json_hint = format!(
            "{system_prompt}\n\n\
             IMPORTANT: You MUST respond with a valid JSON object. No markdown, no code fences, just raw JSON."
        );

        let request = AnthropicRequest {
            model: self.config.model.clone(),
            max_tokens,
            system: vec![SystemBlock {
                r#type: "text".into(),
                text: system_with_json_hint,
                cache_control: CacheControl {
                    r#type: "ephemeral".into(),
                },
            }],
            messages: deduped,
        };

        serde_json::to_string(&request).map_err(|e| e.to_string())
    }

    fn endpoint_path(&self) -> &str {
        "/v1/messages"
    }

    fn auth_headers(&self) -> Vec<String> {
        vec![
            format!("x-api-key: {}", self.config.api_key),
            "anthropic-version: 2023-06-01".to_string(),
            "anthropic-beta: prompt-caching-2024-07-31".to_string(),
        ]
    }

    fn parse_response(&self, body: &str) -> Result<(String, Option<TokenUsage>), String> {
        let resp: AnthropicResponse = serde_json::from_str(body)
            .map_err(|e| format!("failed to parse Anthropic response: {e}\nbody: {body}"))?;

        let usage = resp.usage.map(|u| {
            let cache_tokens = u.cache_creation_input_tokens + u.cache_read_input_tokens;
            TokenUsage {
                prompt_tokens: u.input_tokens + cache_tokens,
                completion_tokens: u.output_tokens,
                total_tokens: u.input_tokens + cache_tokens + u.output_tokens,
            }
        });

        let content = resp
            .content
            .into_iter()
            .next()
            .ok_or("no content blocks in Anthropic response")?
            .text;

        Ok((content, usage))
    }
}
