//! Transaction — pure domain object describing an atomic mutation.
//!
//! `Transaction<()>` for write input, `Transaction<TxMeta>` after commit.

use serde_json::{Map, Value};

use crate::domain::entity::Entity;
use crate::domain::managed::Managed;
use crate::domain::tx_meta::TxMeta;
use crate::io::Slug;

/// Atomic mutation intent — pure domain data.
///
/// `M = ()` before commit (caller input).
/// `M = TxMeta` after commit (with tx_id and branch).
#[derive(Debug, Clone)]
pub struct Transaction<M = ()> {
    pub meta: Map<String, Value>,
    pub context: M,
}

/// Full transaction detail — reconstructed from storage for inspection.
#[derive(Debug, Clone)]
pub struct TransactionDetail {
    /// Transaction metadata.
    pub meta: Map<String, Value>,
    /// Branch slug where the transaction was committed.
    pub branch: Slug,
    /// Transaction provenance.
    pub context: TxMeta,
    /// Entities created in this transaction (with properties).
    pub created: Vec<Entity<TxMeta>>,
    /// Entities updated in this transaction (only changed properties).
    pub updated: Vec<Entity<TxMeta>>,
    /// Entities deleted in this transaction.
    pub deleted: Vec<DeletedEntity>,
    /// Entities explicitly touched without mutation.
    pub touched: Vec<TouchedEntity>,
    /// Managed items created in this transaction.
    pub created_managed: Vec<Managed<TxMeta>>,
    /// Managed items updated in this transaction.
    pub updated_managed: Vec<Managed<TxMeta>>,
}

/// A deleted entity with its deletion reasoning and provenance.
#[derive(Debug, Clone)]
pub struct DeletedEntity {
    pub slug: Slug,
    pub meta: Map<String, Value>,
    pub reasoning: Value,
    pub context: TxMeta,
}

/// An entity that was explicitly touched (marked relevant) without mutation.
#[derive(Debug, Clone)]
pub struct TouchedEntity {
    pub slug: Slug,
    pub reasoning: String,
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
