//! Response DTOs — serialized to YAML/JSON response bodies.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

/// Transaction provenance context.
#[derive(Serialize)]
pub struct TxMetaRes {
    pub tx_id: Uuid,
    pub branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<DateTime<Utc>>,
    /// Epistemic status of the property assertion (per `assertion_statuses`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Provenance of the property assertion — where the knowledge came from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Transaction result.
#[derive(Serialize)]
pub struct TransactionRes {
    pub meta: Map<String, Value>,
    pub context: TxMetaRes,
}

/// Entity property with provenance.
#[derive(Serialize)]
pub struct PropertyValueRes {
    pub property: String,
    pub value: Value,
    pub context: TxMetaRes,
}

/// Entity result.
#[derive(Serialize)]
pub struct EntityRes {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Value>,
    pub properties: Vec<PropertyValueRes>,
    pub meta: Map<String, Value>,
    pub context: TxMetaRes,
}

/// Branch metadata result.
#[derive(Serialize)]
pub struct BranchRes {
    pub slug: String,
    pub parent: String,
    pub meta: Map<String, Value>,
    pub context: TxMetaRes,
}

/// Managed item result.
#[derive(Serialize)]
pub struct ManagedRes {
    pub type_name: String,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub fields: Map<String, Value>,
    pub context: TxMetaRes,
}

/// Full transaction detail result.
#[derive(Serialize)]
pub struct TransactionDetailRes {
    pub meta: Map<String, Value>,
    pub branch: String,
    pub context: TxMetaRes,
    pub created: Vec<EntityRes>,
    pub updated: Vec<EntityRes>,
    pub deleted: Vec<DeletedEntityRes>,
    pub touched: Vec<TouchedEntityRes>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub created_managed: Vec<ManagedRes>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub updated_managed: Vec<ManagedRes>,
}

/// Deleted entity in a transaction detail.
#[derive(Serialize)]
pub struct DeletedEntityRes {
    pub slug: String,
    pub meta: Map<String, Value>,
    pub reasoning: Value,
    pub context: TxMetaRes,
}

/// Touched entity in a transaction detail.
#[derive(Serialize)]
pub struct TouchedEntityRes {
    pub slug: String,
    pub reasoning: String,
}

/// Checkout result.
#[derive(Serialize)]
pub struct CheckoutRes {
    pub branch: String,
    pub created_from_tx: Uuid,
    pub transaction: TransactionRes,
}

/// Similar entity search result entry — full entity + score.
#[derive(Serialize)]
pub struct SimilarEntityRes {
    #[serde(flatten)]
    pub entity: EntityRes,
    pub similarity: f32,
    pub matched_query: usize,
}

/// Entity history entry — entity snapshot at a past tx + what changed + tx meta.
#[derive(Serialize)]
pub struct EntityHistoryEntryRes {
    #[serde(flatten)]
    pub entity: EntityRes,
    pub changed_properties: Vec<String>,
    pub transaction_meta: Map<String, Value>,
}

/// Error response.
#[derive(Serialize)]
pub struct ErrorRes {
    pub error: String,
    pub kind: String,
}
