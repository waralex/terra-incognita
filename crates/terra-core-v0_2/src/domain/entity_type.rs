//! Resolved entity type with its attached properties.

use uuid::Uuid;

use crate::domain::property::Property;

/// Entity type as seen by the caller — resolved from schema entries
/// across the ancestry chain, latest version, hidden filtered.
#[derive(Debug, Clone)]
pub struct EntityType {
    pub id: Uuid,
    pub slug: String,
    pub description: Option<serde_json::Value>,
    pub properties: Vec<Property>,
}
