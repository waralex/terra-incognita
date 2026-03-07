mod store;

pub use store::AssertionStore;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssertionKind {
    Hypothesis,
    Refinement,
}

impl AssertionKind {
    pub fn as_byte(self) -> u8 {
        match self {
            AssertionKind::Hypothesis => 0x00,
            AssertionKind::Refinement => 0x01,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub entity_id: Uuid,
    pub entity_type: String,
    pub kind: AssertionKind,
    pub name: String,
    pub properties: serde_json::Value,
    pub context: serde_json::Value,
}

#[derive(Debug, thiserror::Error)]
pub enum AssertionError {
    #[error("invalid name: {0}")]
    InvalidName(String),

    #[error("entity type not found: {0}")]
    EntityTypeNotFound(String),

    #[error("storage error: {0}")]
    Storage(String),
}
