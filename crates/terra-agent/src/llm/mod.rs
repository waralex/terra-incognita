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

/// A command requested by the LLM (e.g. SQL query).
#[derive(Clone, Serialize)]
pub struct LlmCommand {
    pub reasoning: serde_json::Value,
    #[serde(rename = "type")]
    pub command_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Result from an LLM call.
pub struct LlmResult {
    pub answer: String,
    /// YAML transaction to dispatch (empty string if no transaction fields).
    pub transaction_yaml: String,
    pub usage: Option<TokenUsage>,
    pub commands: Vec<LlmCommand>,
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
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let full_system = format!(
        "{system_prompt}\n\n# Current time\n{now}\n\n# Current branch state\n```yaml\n{branch_state}\n```"
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

    // Parse JSON to extract answer, commands, and transaction fields
    let val: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("LLM returned invalid JSON: {e}\ncontent: {content}"))?;

    let answer = val
        .get("answer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Extract commands
    let commands: Vec<LlmCommand> = val
        .get("commands")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let command_type = c.get("type")?.as_str()?.to_string();
                    Some(LlmCommand {
                        reasoning: c.get("reasoning").cloned().unwrap_or(serde_json::Value::Null),
                        command_type,
                        query: c.get("query").and_then(|v| v.as_str()).map(String::from),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    // Build transaction YAML, stripping non-transaction fields
    let transaction_yaml = if let Some(obj) = val.as_object() {
        let tx_keys = [
            "reasoning", "question", "answer", "timestamp",
            "entity_types", "properties", "attach",
            "hide", "unhide", "introduce", "asserts",
        ];
        let has_mutations = obj.keys().any(|k| {
            matches!(k.as_str(),
                "entity_types" | "properties" | "attach" |
                "hide" | "unhide" | "introduce" | "asserts"
            )
        });

        if has_mutations || obj.contains_key("reasoning") {
            let mut tx: serde_json::Map<String, serde_json::Value> = obj
                .iter()
                .filter(|(k, _)| tx_keys.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            tx.insert("command".into(), serde_json::Value::String("transaction".into()));
            serde_yaml::to_string(&tx)
                .map_err(|e| format!("failed to serialize to YAML: {e}"))?
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    Ok(LlmResult {
        answer,
        transaction_yaml,
        usage,
        commands,
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
