use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use terra_core::assertion::LogEntry;
use terra_core::command::{SessionDetail, TransactionEntityResult};
use terra_core::schema::EntityProperty;

/// Response for entity.create / entity.assert commands.
#[derive(Serialize)]
pub struct AssertedResponse {
    pub tx_id: Uuid,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<LogEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hypotheses: Vec<LogEntry>,
}

/// Response for multi-entity transaction command.
#[derive(Serialize)]
pub struct TransactionResultResponse {
    pub tx_id: Uuid,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub introduce: Vec<TransactionEntityResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asserts: Vec<TransactionEntityResult>,
}

/// Response for property.attach command.
#[derive(Serialize)]
pub struct AttachedResponse {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// Response for entity-type.get — flattened entity type with properties.
#[derive(Serialize)]
pub struct EntityTypeDetailResponse {
    pub id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub properties: Vec<EntityProperty>,
}

/// Slim entity item for entity.list response.
#[derive(Serialize)]
pub struct EntityListItem {
    pub id: Uuid,
    pub slug: String,
}

/// Response for session.get — reshapes entity_types and entities to slim views.
#[derive(Serialize)]
pub struct SessionResponse {
    pub id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub entity_types: Vec<EntityTypeSlim>,
    pub seed_entities: Vec<EntitySlim>,
    pub introduced_entities: Vec<EntitySlim>,
}

/// Slim entity type reference.
#[derive(Serialize)]
pub struct EntityTypeSlim {
    pub slug: String,
}

/// Slim entity reference.
#[derive(Serialize)]
pub struct EntitySlim {
    pub id: Uuid,
    pub slug: String,
}

impl From<SessionDetail> for SessionResponse {
    fn from(d: SessionDetail) -> Self {
        Self {
            id: d.id,
            slug: d.slug,
            description: d.description,
            entity_types: d.entity_types.into_iter().map(|et| EntityTypeSlim { slug: et.slug }).collect(),
            seed_entities: d.seed_entities.into_iter().map(|e| EntitySlim { id: e.id, slug: e.slug }).collect(),
            introduced_entities: d.introduced_entities.into_iter().map(|e| EntitySlim { id: e.id, slug: e.slug }).collect(),
        }
    }
}
