use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

use serde::{Deserialize, Serialize};

/// LLM provider configuration.
pub struct LlmConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl LlmConfig {
    /// Loads config from environment variables.
    ///
    /// Required: `TERRA_LLM_API_KEY`
    /// Optional: `TERRA_LLM_BASE_URL` (default: OpenAI), `TERRA_LLM_MODEL` (default: gpt-4o)
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("TERRA_LLM_API_KEY").ok()?;
        let base_url = std::env::var("TERRA_LLM_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let model = std::env::var("TERRA_LLM_MODEL")
            .unwrap_or_else(|_| "gpt-4o".into());
        Some(Self { api_key, base_url, model })
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    response_format: ResponseFormat,
}

#[derive(Serialize)]
struct ResponseFormat {
    r#type: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
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

#[derive(Deserialize, Clone)]
struct Usage {
    prompt_tokens: usize,
    completion_tokens: usize,
    total_tokens: usize,
}

/// Token usage from the LLM API response.
#[derive(Clone)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

/// Result from an LLM call: the answer text and the raw transaction JSON.
pub struct LlmResult {
    pub answer: String,
    pub transaction_json: String,
    pub usage: Option<TokenUsage>,
}

/// Rough token estimate: ~4 chars per token for English/mixed content.
fn estimate_tokens(text: &str) -> usize {
    (text.len() + 3) / 4
}

/// Max tokens we allow in a single request payload (input side).
/// ~120k chars ≈ 30k tokens — safe margin for most models.
const MAX_PAYLOAD_BYTES: usize = 120_000;

/// Calls the LLM API and extracts answer + transaction from the response.
///
/// The LLM is expected to return a JSON object that IS a terra transaction,
/// with `answer` and `reasoning` fields.
pub fn call_llm(
    config: &LlmConfig,
    system_prompt: &str,
    branch_state: &str,
    history: &[ChatMessage],
    user_message: &str,
) -> Result<LlmResult, String> {
    let mut messages = vec![
        ChatMessage {
            role: "system".into(),
            content: format!("{system_prompt}\n\n# Current branch state\n```yaml\n{branch_state}\n```"),
        },
    ];
    messages.extend_from_slice(history);
    messages.push(ChatMessage {
        role: "user".into(),
        content: user_message.into(),
    });

    let request = ChatRequest {
        model: config.model.clone(),
        messages,
        response_format: ResponseFormat {
            r#type: "json_object".into(),
        },
    };

    let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;

    if body.len() > MAX_PAYLOAD_BYTES {
        let tokens = estimate_tokens(&body);
        return Err(format!(
            "payload too large: ~{tokens} tokens ({} bytes). Reduce history or branch state.",
            body.len()
        ));
    }

    let response_body = http_post(&config.base_url, "/chat/completions", &config.api_key, &body)?;

    let resp: ChatResponse = serde_json::from_str(&response_body)
        .map_err(|e| format!("failed to parse LLM response: {e}\nbody: {response_body}"))?;

    let usage = resp.usage.map(|u| TokenUsage {
        prompt_tokens: u.prompt_tokens,
        completion_tokens: u.completion_tokens,
        total_tokens: u.total_tokens,
    });

    let content = resp.choices.into_iter()
        .next()
        .ok_or("no choices in LLM response")?
        .message
        .content;

    // Parse the JSON to extract answer, then use the whole thing as transaction
    let val: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("LLM returned invalid JSON: {e}\ncontent: {content}"))?;

    let answer = val.get("answer")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Build a YAML transaction from the JSON, adding command: transaction
    let mut tx = val.clone();
    if let Some(obj) = tx.as_object_mut() {
        obj.insert("command".into(), serde_json::Value::String("transaction".into()));
    }
    let transaction_yaml = serde_yaml::to_string(&tx)
        .map_err(|e| format!("failed to serialize transaction to YAML: {e}"))?;

    Ok(LlmResult {
        answer,
        transaction_json: transaction_yaml,
        usage,
    })
}

/// Retries LLM call up to `max_retries` times, feeding back errors.
///
/// Bounded: exactly `max_retries + 1` attempts max. No infinite loop.
pub fn call_llm_with_retry(
    config: &LlmConfig,
    system_prompt: &str,
    branch_state: &str,
    history: &[ChatMessage],
    user_message: &str,
    max_retries: usize,
) -> Result<LlmResult, String> {
    let mut last_error = String::new();

    for attempt in 0..=max_retries {
        let msgs = if attempt == 0 {
            history.to_vec()
        } else {
            let mut m = history.to_vec();
            m.push(ChatMessage {
                role: "user".into(),
                content: user_message.into(),
            });
            m.push(ChatMessage {
                role: "system".into(),
                content: format!("Error from previous attempt: {last_error}\nPlease fix and return a valid transaction."),
            });
            m
        };

        match call_llm(config, system_prompt, branch_state, &msgs, user_message) {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = e;
            }
        }
    }

    Err(format!("LLM failed after {} attempts: {}", max_retries + 1, last_error))
}

/// Minimal HTTP POST using raw TcpStream + native-tls (or plain TCP).
/// Parses URL, sends request, reads response.
fn http_post(base_url: &str, path: &str, api_key: &str, body: &str) -> Result<String, String> {
    let url = format!("{base_url}{path}");

    // Parse URL
    let is_https = url.starts_with("https://");
    let without_scheme = url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("invalid URL scheme")?;

    let (host_port, request_path) = without_scheme.split_once('/')
        .map(|(h, p)| (h, format!("/{p}")))
        .unwrap_or((without_scheme, path.to_string()));

    let (host, port) = if host_port.contains(':') {
        let (h, p) = host_port.rsplit_once(':').unwrap();
        (h, p.parse::<u16>().map_err(|e| e.to_string())?)
    } else if is_https {
        (host_port, 443)
    } else {
        (host_port, 80)
    };

    let request = format!(
        "POST {request_path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Authorization: Bearer {api_key}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );

    if is_https {
        // Use native-tls
        let connector = native_tls::TlsConnector::new()
            .map_err(|e| format!("TLS error: {e}"))?;
        let stream = TcpStream::connect((host, port))
            .map_err(|e| format!("connection error: {e}"))?;
        let mut tls_stream = connector.connect(host, stream)
            .map_err(|e| format!("TLS handshake error: {e}"))?;
        tls_stream.write_all(request.as_bytes())
            .map_err(|e| format!("write error: {e}"))?;
        tls_stream.flush().map_err(|e| format!("flush error: {e}"))?;
        read_http_response(BufReader::new(tls_stream))
    } else {
        let mut stream = TcpStream::connect((host, port))
            .map_err(|e| format!("connection error: {e}"))?;
        stream.write_all(request.as_bytes())
            .map_err(|e| format!("write error: {e}"))?;
        stream.flush().map_err(|e| format!("flush error: {e}"))?;
        read_http_response(BufReader::new(stream))
    }
}

fn read_http_response<R: BufRead>(mut reader: R) -> Result<String, String> {
    // Read status line
    let mut status_line = String::new();
    reader.read_line(&mut status_line).map_err(|e| e.to_string())?;

    // Read headers
    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_lowercase();
        if lower.starts_with("content-length:") {
            content_length = lower.split(':').nth(1)
                .and_then(|v| v.trim().parse().ok());
        }
        if lower.contains("transfer-encoding: chunked") {
            chunked = true;
        }
    }

    // Read body
    let body = if let Some(len) = content_length {
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
        String::from_utf8(buf).map_err(|e| e.to_string())?
    } else if chunked {
        read_chunked_body(&mut reader)?
    } else {
        let mut buf = String::new();
        reader.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        buf
    };

    // Check HTTP status
    if !status_line.contains("200") {
        return Err(format!("HTTP error: {}\n{}", status_line.trim(), body));
    }

    Ok(body)
}

fn read_chunked_body<R: BufRead>(reader: &mut R) -> Result<String, String> {
    let mut body = String::new();
    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line).map_err(|e| e.to_string())?;
        let size = usize::from_str_radix(size_line.trim(), 16)
            .map_err(|e| format!("invalid chunk size: {e}"))?;
        if size == 0 {
            break;
        }
        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk).map_err(|e| e.to_string())?;
        body.push_str(&String::from_utf8(chunk).map_err(|e| e.to_string())?);
        // Read trailing \r\n
        let mut crlf = String::new();
        reader.read_line(&mut crlf).map_err(|e| e.to_string())?;
    }
    Ok(body)
}
