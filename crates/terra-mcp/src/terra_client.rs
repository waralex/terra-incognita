//! HTTP client to the terra-server daemon.
//!
//! The MCP server owns the RocksDB store indirectly: it never opens it,
//! it forwards every operation to the long-running daemon over `POST /query`.
//! This keeps the single-writer invariant intact and lets many MCP instances
//! (one per session) share one store.

use serde_json::Value;

/// Default endpoint of a locally installed terra-server.
const DEFAULT_URL: &str = "http://127.0.0.1:7373/query";

/// A thin `POST /query` client for the terra daemon.
pub struct TerraClient {
    url: String,
}

impl TerraClient {
    /// Build a client, reading the endpoint from `TERRA_URL` or falling back
    /// to the local default.
    pub fn from_env() -> Self {
        let url = std::env::var("TERRA_URL").unwrap_or_else(|_| DEFAULT_URL.to_string());
        Self { url }
    }

    /// Send a command body to the daemon and return the parsed JSON response.
    ///
    /// terra answers a 4xx with a JSON `{ error, kind }` body; that body is
    /// returned as-is (not an `Err`) so the caller can surface `kind`. `Err`
    /// is reserved for transport failures (daemon down, unreadable response).
    pub fn query(&self, body: Value) -> Result<Value, String> {
        match ureq::post(&self.url).send_json(body) {
            Ok(resp) => resp
                .into_json::<Value>()
                .map_err(|e| format!("failed to read response: {e}")),
            Err(ureq::Error::Status(_, resp)) => resp
                .into_json::<Value>()
                .map_err(|e| format!("failed to read error response: {e}")),
            Err(ureq::Error::Transport(t)) => {
                Err(format!("cannot reach terra at {}: {t}", self.url))
            }
        }
    }
}
