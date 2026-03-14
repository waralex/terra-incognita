//! Resolved property of an entity type.

use uuid::Uuid;

/// Property as seen by the caller — latest version from ancestry chain.
#[derive(Debug, Clone)]
pub struct Property {
    pub id: Uuid,
    pub slug: String,
    pub description: Option<serde_json::Value>,
}
