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

/// Checkout result.
#[derive(Serialize)]
pub struct CheckoutRes {
    pub branch: String,
    pub created_from_tx: Uuid,
    pub transaction: TransactionRes,
}

/// Similar entity search result entry.
#[derive(Serialize)]
pub struct SimilarEntityRes {
    pub slug: String,
    pub similarity: f32,
}

/// Error response.
#[derive(Serialize)]
pub struct ErrorRes {
    pub error: String,
    pub kind: String,
}
