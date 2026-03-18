//! Entity history entry — a snapshot of an entity at a point in time.

use serde_json::{Map, Value};

use crate::domain::entity::Entity;
use crate::domain::tx_meta::TxMeta;
use crate::io::slug::Slug;

/// A single history entry: entity snapshot + what changed in this transaction.
#[derive(Debug)]
pub struct EntityHistoryEntry {
    /// Full entity snapshot at this transaction point.
    pub entity: Entity<TxMeta>,
    /// Property slugs that changed in this transaction.
    pub changed_properties: Vec<Slug>,
    /// Transaction metadata (reasoning, etc.).
    pub transaction_meta: Map<String, Value>,
}
