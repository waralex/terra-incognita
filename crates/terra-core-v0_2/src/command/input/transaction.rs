//! TransactionInput — command describing an atomic mutation.
//!
//! Built incrementally by the caller, then passed to ExecuteTransaction.
//! All fields are private — populated through builder methods.

use serde_json::{Map, Value};

/// Atomic mutation command — all operations to execute in a single transaction.
///
/// Created via `TransactionInput::new(meta)`, then populated with
/// builder methods. Passed to `ExecuteTransaction` for execution.
pub struct TransactionInput {
    pub(crate) meta: Map<String, Value>,
}

impl TransactionInput {
    /// Start building a transaction with the given metadata.
    pub fn new(meta: Map<String, Value>) -> Self {
        Self { meta }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_empty() {
        let mut meta = Map::new();
        meta.insert("reasoning".into(), serde_json::json!("test"));

        let input = TransactionInput::new(meta);
        assert_eq!(input.meta["reasoning"], "test");
    }
}
