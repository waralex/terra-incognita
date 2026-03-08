use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// Registered entity type in the schema registry.
#[derive(Debug, Clone, Serialize)]
pub struct EntityType {
    pub id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}
