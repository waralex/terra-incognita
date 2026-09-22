//! Experimental JSONL process bridge for the Markdown adapter; owns a dedicated DB.
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use terra_core::documents::{DocumentError, DocumentStore, Operation};
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    LatestTransaction,
    Transaction {
        tx: Uuid,
    },
    Changes {
        tx: Uuid,
    },
    Transact {
        author: String,
        reason: String,
        operations: Vec<Operation>,
    },
    Subtree {
        root: Uuid,
        at: Option<Uuid>,
        depth: usize,
        max_nodes: usize,
    },
    Get {
        id: Uuid,
        at: Option<Uuid>,
    },
    History {
        id: Uuid,
        before: Option<Uuid>,
        limit: usize,
    },
}
fn execute(store: &DocumentStore, r: Request) -> Result<Value, DocumentError> {
    match r {
        Request::LatestTransaction => Ok(json!(store.latest_transaction()?)),
        Request::Transaction { tx } => Ok(json!(store.transaction(tx)?)),
        Request::Changes { tx } => Ok(json!(store.changed_blocks(tx)?)),
        Request::Transact {
            author,
            reason,
            operations,
        } => Ok(json!({"tx":store.transact(&author,&reason,operations)?})),
        Request::Subtree {
            root,
            at,
            depth,
            max_nodes,
        } => Ok(json!(store.subtree(root, at, depth, max_nodes)?)),
        Request::Get { id, at } => Ok(json!(store.get(id, at)?)),
        Request::History { id, before, limit } => Ok(json!(store.history(id, before, limit)?)),
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: document-bridge NEW_DB_DIRECTORY")?;
    let store = DocumentStore::open(std::path::Path::new(&path))?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line?;
        let result = if line.len() > 4_000_000 {
            Err(DocumentError::Limit("request exceeds 4MB".into()))
        } else {
            serde_json::from_str::<Request>(&line)
                .map_err(|e| DocumentError::InvalidInput(e.to_string()))
                .and_then(|r| execute(&store, r))
        };
        let value = match result {
            Ok(value) => json!({"ok":value}),
            Err(error) => {
                let kind = match &error {
                    DocumentError::Conflict(_) => "conflict",
                    DocumentError::InvalidInput(_) => "invalid_input",
                    DocumentError::Limit(_) => "limit",
                    DocumentError::Storage(_) => "storage",
                };
                json!({"error":{"kind":kind,"message":error.to_string()}})
            }
        };
        serde_json::to_writer(&mut out, &value)?;
        writeln!(out)?;
        out.flush()?;
    }
    Ok(())
}
