//! terra-mcp — an MCP server exposing terra as Claude's work memory.
//!
//! Speaks MCP over stdio (newline-delimited JSON-RPC 2.0) and forwards every
//! tool call to a running terra-server daemon over HTTP. It is a thin,
//! stateless translator: it holds no state and never opens the store.

mod jsonrpc;
mod server;
mod terra_client;
mod tools;

use std::io::{BufRead, Write};

use jsonrpc::Request;
use server::McpServer;

fn main() {
    let server = McpServer::new();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(req) => server.handle(&req),
            Err(e) => Some(jsonrpc::error(None, -32700, &format!("parse error: {e}"))),
        };
        if let Some(response) = response {
            let _ = writeln!(out, "{}", response);
            let _ = out.flush();
        }
    }
}
