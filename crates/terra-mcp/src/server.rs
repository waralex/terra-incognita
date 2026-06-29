//! MCP method dispatch — turns JSON-RPC requests into terra commands.

use serde_json::{json, Value};

use crate::jsonrpc::{error, success, Request};
use crate::terra_client::TerraClient;
use crate::tools;

/// Protocol version advertised when the client does not propose one.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server-level memory protocol, injected once by the client at connect time
/// (no per-turn accumulation). Travels with the server to any project it is
/// connected in.
const INSTRUCTIONS: &str = "\
These tools are your cross-session work memory. Use them like human memory — \
associations and directions, not a verbatim cache.\n\
- Recall at task start: before non-trivial work, `recall`/`grep` what is \
already known. Treat code-sourced memory as \"verify, don't trust\" — it ages.\n\
- Record durable findings as you reach them: when you derive something worth \
not re-deriving (a code seam, where-to-look, a decision, a user preference), \
`remember`/`link`/`note` it at the right altitude (the map and conclusion, not \
line numbers), with `source` and `status`.\n\
- Consolidate by writing a `status: fact` assertion — it supersedes earlier \
observations on that property (kept in history). No separate consolidate step.\n\
- Don't store what's volatile or already in code/git. Memory is for what the \
repo can't tell you, stable enough to outlive the session.\n\
- Slugs are your internal addressing (dotted namespace, e.g. cube.cubestore); \
pass `project` to scope an entity.";

/// Stateless MCP server: every tool call is forwarded to the terra daemon.
pub struct McpServer {
    terra: TerraClient,
}

impl McpServer {
    /// Build a server with a daemon client from the environment.
    pub fn new() -> Self {
        Self {
            terra: TerraClient::from_env(),
        }
    }

    /// Handle one message. Returns `Some(response)` for requests, `None` for
    /// notifications (which expect no reply).
    pub fn handle(&self, req: &Request) -> Option<Value> {
        if req.is_notification() {
            return None;
        }
        let id = req.id.clone();
        let response = match req.method.as_str() {
            "initialize" => success(id, self.initialize(&req.params)),
            "tools/list" => success(id, json!({ "tools": tools::list() })),
            "tools/call" => success(id, self.call_tool(&req.params)),
            "ping" => success(id, json!({})),
            other => error(id, -32601, &format!("method not found: {other}")),
        };
        Some(response)
    }

    /// Build the `initialize` result, echoing the client's protocol version.
    fn initialize(&self, params: &Value) -> Value {
        let version = params
            .get("protocolVersion")
            .and_then(|v| v.as_str())
            .unwrap_or(PROTOCOL_VERSION);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "terra-mcp", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS
        })
    }

    /// Execute a `tools/call`: map to a terra command, run it, wrap the result.
    ///
    /// Tool-level failures (bad arguments, terra errors) are returned as an
    /// `isError` result rather than a protocol error, per MCP convention.
    fn call_tool(&self, params: &Value) -> Value {
        let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let args = params.get("arguments").cloned().unwrap_or(json!({}));

        let body = match tools::build_query(name, &args) {
            Ok(body) => body,
            Err(e) => return tool_error(&e),
        };
        match self.terra.query(body) {
            Ok(resp) if resp.get("error").is_some() => tool_error(&describe_terra_error(&resp)),
            Ok(resp) => tool_text(&pretty(&resp)),
            Err(e) => tool_error(&e),
        }
    }
}

/// A successful tool result carrying a text block.
fn tool_text(text: &str) -> Value {
    json!({ "content": [ { "type": "text", "text": text } ], "isError": false })
}

/// A failed tool result carrying an error message.
fn tool_error(message: &str) -> Value {
    json!({ "content": [ { "type": "text", "text": message } ], "isError": true })
}

/// Render a terra `{ error, kind }` body into a one-line message.
fn describe_terra_error(resp: &Value) -> String {
    let msg = resp
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("error");
    match resp.get("kind").and_then(|v| v.as_str()) {
        Some(kind) => format!("{msg} ({kind})"),
        None => msg.to_string(),
    }
}

/// Pretty-print JSON, falling back to compact on failure.
fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}
