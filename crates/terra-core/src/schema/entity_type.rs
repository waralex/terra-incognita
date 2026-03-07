use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct EntityType {
    pub id: Uuid,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}
