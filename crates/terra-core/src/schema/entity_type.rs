use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct EntityType {
    pub id: Uuid,
    pub slug: String,
    pub created_at: DateTime<Utc>,
}
