mod store;

pub use store::AssertionStore;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Whether an assertion is tentative or a convergence point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssertionKind {
    /// Tentative claim; multiple can coexist for the same property.
    Hypothesis,
    /// Convergence point marking a decision amid uncertainty.
    Refinement,
}

impl AssertionKind {
    /// Returns the byte representation used in storage keys.
    pub fn as_byte(self) -> u8 {
        match self {
            AssertionKind::Hypothesis => 0x00,
            AssertionKind::Refinement => 0x01,
        }
    }
}

/// A single entry in the assertion log, representing an entity creation event.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub entity_id: Uuid,
    pub entity_type: Option<String>,
    pub name: String,
    pub context: serde_json::Value,
}

/// Input for creating an entity (single item in a batch).
pub struct EntityInput<'a> {
    pub name: &'a str,
    pub entity_type: Option<&'a str>,
    pub context: serde_json::Value,
}

/// Errors from assertion store operations.
#[derive(Debug, thiserror::Error)]
pub enum AssertionError {
    /// Entity name failed slug validation.
    #[error("invalid name: {0}")]
    InvalidName(String),

    /// Referenced entity type does not exist in the schema.
    #[error("entity type not found: {0}")]
    EntityTypeNotFound(String),

    /// Error within a batch operation, with the index of the failing item.
    #[error("batch item {index}: {source}")]
    BatchItemError {
        index: usize,
        source: Box<AssertionError>,
    },

    /// Underlying RocksDB or serialization error.
    #[error("storage error: {0}")]
    Storage(String),
}
