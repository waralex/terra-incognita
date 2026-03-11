use serde::Serialize;
use uuid::Uuid;

use terra_core::command::{BranchDetail, TransactionEntityResult};
use terra_core::schema::{EntityProperty, EntityType};

/// Response for the unified transaction command.
#[derive(Serialize)]
pub struct TransactionResultResponse {
    pub tx_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entity_types: Vec<EntityType>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<EntityProperty>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub introduce: Vec<TransactionEntityResult>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub asserts: Vec<TransactionEntityResult>,
}

/// Slim entity item for entity.list response.
#[derive(Serialize)]
pub struct EntityListItem {
    pub id: Uuid,
    pub slug: String,
}

/// Response for branch.get / branch.create.
#[derive(Serialize)]
pub struct BranchResponse {
    pub id: Uuid,
    pub slug: String,
    pub reasoning: serde_json::Value,
    pub created_from_tx: Uuid,
    pub parent_id: Uuid,
}

impl From<BranchDetail> for BranchResponse {
    fn from(d: BranchDetail) -> Self {
        Self {
            id: d.id,
            slug: d.slug,
            reasoning: d.reasoning,
            created_from_tx: d.created_from_tx,
            parent_id: d.parent_id,
        }
    }
}
