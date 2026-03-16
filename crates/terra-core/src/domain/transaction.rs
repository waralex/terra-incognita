//! Transaction — pure domain object describing an atomic mutation.
//!
//! `Transaction<()>` for write input, `Transaction<TxMeta>` after commit.

use serde_json::{Map, Value};

/// Atomic mutation intent — pure domain data.
///
/// `M = ()` before commit (caller input).
/// `M = TxMeta` after commit (with tx_id and branch).
#[derive(Debug, Clone)]
pub struct Transaction<M = ()> {
    pub meta: Map<String, Value>,
    pub context: M,
}

impl Transaction<()> {
    /// Create a new transaction (before commit).
    pub fn new(meta: Map<String, Value>) -> Self {
        Self { meta, context: () }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_transaction() {
        let mut meta = Map::new();
        meta.insert("reasoning".into(), serde_json::json!("test"));

        let tx = Transaction::new(meta);
        assert_eq!(tx.meta["reasoning"], "test");
    }
}
