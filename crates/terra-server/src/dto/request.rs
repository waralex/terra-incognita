//! Request DTOs — deserialized from YAML/JSON request bodies.

use serde::Deserialize;
use serde_json::{Map, Value};
use uuid::Uuid;

fn default_branch() -> String {
    "main".into()
}

fn default_limit() -> usize {
    50
}

/// Top-level envelope: every request carries `command` and `branch`.
#[derive(Deserialize)]
pub struct CommandEnvelope {
    pub command: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(flatten)]
    pub body: Value,
}

/// `transaction` command body.
#[derive(Deserialize)]
pub struct TransactionReq {
    pub meta: Map<String, Value>,
    #[serde(default)]
    pub create: Vec<EntityReq>,
    #[serde(default)]
    pub update: Vec<EntityReq>,
    #[serde(default)]
    pub create_managed: Vec<ManagedReq>,
    #[serde(default)]
    pub update_managed: Vec<ManagedReq>,
    #[serde(default)]
    pub delete: Vec<DeleteReq>,
    #[serde(default)]
    pub touch: Vec<TouchReq>,
}

#[derive(Deserialize)]
pub struct EntityReq {
    pub slug: String,
    pub description: Option<Value>,
    #[serde(default)]
    pub properties: Vec<PropertyValueReq>,
    #[serde(default)]
    pub meta: Map<String, Value>,
}

#[derive(Deserialize)]
pub struct PropertyValueReq {
    pub property: String,
    pub value: Value,
}

#[derive(Deserialize)]
pub struct TouchReq {
    pub entity: String,
    pub reasoning: String,
}

#[derive(Deserialize)]
pub struct DeleteReq {
    pub entity: String,
    pub reasoning: Value,
}

#[derive(Deserialize)]
pub struct ManagedReq {
    pub type_name: String,
    pub slug: String,
    pub state: Option<String>,
    #[serde(default)]
    pub fields: Map<String, Value>,
}

/// `checkout` command body.
#[derive(Deserialize)]
pub struct CheckoutReq {
    pub slug: String,
    pub meta: Map<String, Value>,
    pub created_from_tx: Option<Uuid>,
    pub transaction: TransactionReq,
}

/// `transactions.list` command body.
#[derive(Deserialize)]
pub struct ListTransactionsReq {
    pub at_tx: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// `entities.touched` command body.
#[derive(Deserialize)]
pub struct TouchedEntitiesReq {
    pub at_tx: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// `managed.list` command body.
#[derive(Deserialize)]
pub struct ListManagedReq {
    pub at_tx: Option<Uuid>,
}

/// `entities.similar` command body.
#[derive(Deserialize)]
pub struct SimilarEntitiesReq {
    pub queries: Vec<Value>,
    pub at_tx: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub min_similarity: f32,
}
