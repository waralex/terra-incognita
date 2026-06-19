//! Minimal JSON-RPC 2.0 types for the MCP stdio transport.
//!
//! MCP messages are newline-delimited JSON-RPC objects. A message with an
//! `id` is a request (expects a response); one without is a notification.

use serde::Deserialize;
use serde_json::{json, Value};

/// An incoming JSON-RPC message (request or notification).
#[derive(Debug, Deserialize)]
pub struct Request {
    /// Present on requests, absent on notifications.
    #[serde(default)]
    pub id: Option<Value>,
    /// The RPC method name (e.g. `tools/call`).
    pub method: String,
    /// Method parameters; defaults to null when omitted.
    #[serde(default)]
    pub params: Value,
}

impl Request {
    /// Whether this message is a notification (no `id`, no response expected).
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// Build a successful JSON-RPC response for the given request id.
pub fn success(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Build a JSON-RPC error response for the given request id.
pub fn error(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
