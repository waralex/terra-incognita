mod http;

pub mod anthropic;
pub mod openai;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use http::{http_post, log_raw};

/// LLM provider configuration shared by all providers.
pub struct LlmProviderConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub log_path: Option<PathBuf>,
}

/// Trait for LLM API providers (OpenAI, Anthropic, etc.).
pub trait LlmProvider {
    /// Returns the provider configuration.
    fn config(&self) -> &LlmProviderConfig;

    /// Builds the JSON request body for this provider.
    fn build_request_body(
        &self,
        system_prompt: &str,
        messages: &[ChatMessage],
        max_tokens: usize,
    ) -> Result<String, String>;

    /// Returns the API endpoint path (e.g. `/chat/completions`).
    fn endpoint_path(&self) -> &str;

    /// Returns provider-specific HTTP headers (auth, version, etc.).
    fn auth_headers(&self) -> Vec<String>;

    /// Parses the response body into (content_text, optional_usage).
    fn parse_response(&self, body: &str) -> Result<(String, Option<TokenUsage>), String>;
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Token usage from the LLM API response.
#[derive(Clone)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Result from an LLM call: the answer text and the raw transaction YAML.
pub struct LlmResult {
    pub answer: String,
    pub transaction_json: String,
    pub usage: Option<TokenUsage>,
}

/// Max output tokens for LLM response.
const MAX_OUTPUT_TOKENS: usize = 4096;

/// Rough token estimate: ~4 chars per token for English/mixed content.
pub fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Max tokens we allow in a single request payload (input side).
const MAX_PAYLOAD_BYTES: usize = 120_000;

/// Calls the LLM API and extracts answer + transaction from the response.
pub fn call_llm(
    provider: &dyn LlmProvider,
    system_prompt: &str,
    branch_state: &str,
    user_message: &str,
) -> Result<LlmResult, String> {
    let full_system = format!(
        "{system_prompt}\n\n# Current branch state\n```yaml\n{branch_state}\n```"
    );

    let messages = vec![ChatMessage {
        role: "user".into(),
        content: user_message.into(),
    }];

    let body = provider.build_request_body(&full_system, &messages, MAX_OUTPUT_TOKENS)?;

    if body.len() > MAX_PAYLOAD_BYTES {
        let tokens = estimate_tokens(&body);
        return Err(format!(
            "payload too large: ~{tokens} tokens ({} bytes). Reduce history or branch state.",
            body.len()
        ));
    }

    let config = provider.config();
    log_raw(&config.log_path, "REQUEST", &body);

    let headers = provider.auth_headers();
    let response_body =
        http_post(&config.base_url, provider.endpoint_path(), &headers, &body)?;

    log_raw(&config.log_path, "RESPONSE", &response_body);

    let (content, usage) = provider.parse_response(&response_body)?;

    // Parse JSON to extract answer, then convert to YAML transaction
    let val: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("LLM returned invalid JSON: {e}\ncontent: {content}"))?;

    let answer = val
        .get("answer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut tx = val.clone();
    if let Some(obj) = tx.as_object_mut() {
        obj.insert(
            "command".into(),
            serde_json::Value::String("transaction".into()),
        );
    }
    let transaction_yaml =
        serde_yaml::to_string(&tx).map_err(|e| format!("failed to serialize to YAML: {e}"))?;

    Ok(LlmResult {
        answer,
        transaction_json: transaction_yaml,
        usage,
    })
}

/// Retries LLM call up to `max_retries` times, feeding back errors.
pub fn call_llm_with_retry(
    provider: &dyn LlmProvider,
    system_prompt: &str,
    branch_state: &str,
    user_message: &str,
    max_retries: usize,
) -> Result<LlmResult, String> {
    let mut last_error = String::new();

    for attempt in 0..=max_retries {
        let msg = if attempt == 0 {
            user_message.to_string()
        } else {
            format!(
                "{user_message}\n\n[Error from previous attempt: {last_error}\nPlease fix and return a valid transaction.]"
            )
        };

        match call_llm(provider, system_prompt, branch_state, &msg) {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = e;
            }
        }
    }

    Err(format!(
        "LLM failed after {} attempts: {}",
        max_retries + 1,
        last_error
    ))
}
