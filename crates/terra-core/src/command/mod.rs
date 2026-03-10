mod assert_entity;
pub(crate) mod branch;
pub mod branch_state;
pub mod execute;

pub use assert_entity::AssertEntityError;
pub use branch_state::{BranchState, BranchStateError};
pub use execute::execute;
pub use branch::{BranchCommandError, BranchDetail, BranchSummary};

use serde::Serialize;

use crate::assertion::{EntityError, EntityRecord, LogEntry, LogError, PropertyValue, Transaction, WriterError};
use crate::schema::{EntityProperty, EntityType, ValueType};
use crate::schema::SchemaError;

use std::collections::HashMap;
use uuid::Uuid;

/// Domain command — plain enum without serde.
///
/// All writes go through `Transaction` (or `CreateBranch`).
/// Read variants return data without mutation.
pub enum Command {
    /// List all registered entity types.
    ListEntityTypes,
    /// List properties, optionally filtered by entity type.
    ListProperties { entity_type: Option<String> },
    /// Unified write command: schema creation, attachments, visibility, entity introduction and assertions.
    Transaction(TransactionInput),
    /// List all entities (slug + uuid).
    ListEntities,
    /// Create a new branch.
    CreateBranch(CreateBranchInput),
    /// Get branch by slug with resolved references.
    GetBranch { slug: String },
    /// List all branches.
    ListBranches,
    /// List all entries in the fact log.
    ListLog,
    /// Full branch state snapshot. Optional `at_tx` for time travel.
    BranchState {
        slug: String,
        last_transactions: usize,
        at_tx: Option<Uuid>,
    },
}

/// Input for creating an entity type.
pub struct CreateEntityType {
    pub slug: String,
    pub description: Option<String>,
    /// Property slugs to attach to the new entity type.
    pub properties: Vec<String>,
}

/// Input for creating a property.
pub struct CreateProperty {
    pub slug: String,
    pub value_type: ValueType,
    pub description: Option<String>,
    /// Entity type slugs to attach this property to.
    pub entity_types: Vec<String>,
}

/// Input for attaching a property to an entity type.
pub struct AttachProperty {
    pub entity_type: String,
    pub property: String,
}

/// A single fact or hypothesis: entity type + properties + reasoning.
pub struct AssertionItem {
    /// Entity type slug (determines which properties are valid).
    pub entity_type: String,
    /// Property slug → typed value.
    pub properties: HashMap<String, PropertyValue>,
    /// Per-assertion reasoning: why this specific value.
    pub reasoning: serde_json::Value,
}

/// Input for the unified transaction command.
///
/// All write operations are expressed here: schema creation, property attachment,
/// visibility changes, entity introduction, and assertions on existing entities.
/// Processed in order: properties → entity_types → attach → hide/unhide → introduce → asserts.
pub struct TransactionInput {
    /// Transaction-level reasoning: why this batch of operations.
    pub reasoning: serde_json::Value,
    /// Entity types to create (processed first).
    pub entity_types: Vec<CreateEntityType>,
    /// Properties to create (processed second).
    pub properties: Vec<CreateProperty>,
    /// Property-to-entity-type attachments (processed third).
    pub attach: Vec<AttachProperty>,
    /// Items to hide on the current branch.
    pub hide: HideUnhideInput,
    /// Items to unhide on the current branch.
    pub unhide: HideUnhideInput,
    /// New entities to introduce (created after schema operations).
    pub introduce: Vec<IntroduceItem>,
    /// Assertions on existing entities (processed after introduces).
    pub asserts: Vec<AssertItem>,
}

/// Items to hide or unhide on a branch, referenced by slug.
pub struct HideUnhideInput {
    pub entities: Vec<String>,
    pub entity_types: Vec<String>,
    pub properties: Vec<String>,
}

impl Default for HideUnhideInput {
    fn default() -> Self {
        Self {
            entities: vec![],
            entity_types: vec![],
            properties: vec![],
        }
    }
}

/// A new entity to introduce in a transaction.
pub struct IntroduceItem {
    /// Entity slug.
    pub entity: String,
    /// Entity description.
    pub description: Option<String>,
    /// Facts for this entity.
    pub facts: Vec<AssertionItem>,
    /// Hypotheses for this entity.
    pub hypotheses: Vec<AssertionItem>,
}

/// Assertions on an existing entity in a transaction.
pub struct AssertItem {
    /// Entity slug (must exist or be introduced earlier in the same transaction).
    pub entity: String,
    /// Facts for this entity.
    pub facts: Vec<AssertionItem>,
    /// Hypotheses for this entity.
    pub hypotheses: Vec<AssertionItem>,
}

/// Result of a multi-entity transaction: per-entity facts and hypotheses.
#[derive(Debug, Serialize)]
pub struct TransactionEntityResult {
    pub entity_id: Uuid,
    #[serde(rename = "entity")]
    pub entity_slug: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<LogEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hypotheses: Vec<LogEntry>,
}

/// Input for creating a branch.
pub struct CreateBranchInput {
    pub slug: String,
    pub reasoning: serde_json::Value,
    /// Parent branch slug ("main" or empty for main).
    pub parent: String,
    /// Transaction UUID to branch from. `None` = branch from HEAD.
    pub from_tx: Option<Uuid>,
}

/// Result of executing a command.
#[derive(Debug)]
pub enum CommandResult {
    /// Created or listed entity types.
    EntityTypes(Vec<EntityType>),
    /// Created or listed properties.
    Properties(Vec<EntityProperty>),
    /// Unified transaction result.
    TransactionResult {
        transaction: Transaction,
        entity_types: Vec<EntityType>,
        properties: Vec<EntityProperty>,
        attached_count: usize,
        introduced: Vec<TransactionEntityResult>,
        asserted: Vec<TransactionEntityResult>,
    },
    /// List of entities (slug + uuid).
    EntityList(Vec<EntityRecord>),
    /// Branch created or retrieved with resolved references.
    Branch(BranchDetail),
    /// List of branch summaries.
    BranchList(Vec<BranchSummary>),
    /// Full assertion log.
    LogEntries(Vec<LogEntry>),
    /// Full branch state snapshot.
    BranchState(branch_state::BranchState),
}

/// Errors from command execution.
#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    /// Schema registry error (validation, duplicates, not found).
    #[error(transparent)]
    Schema(#[from] SchemaError),

    /// Log storage error.
    #[error(transparent)]
    Log(#[from] LogError),

    /// Entity operation error.
    #[error(transparent)]
    Entity(#[from] EntityError),

    /// Assertion writer error.
    #[error(transparent)]
    Writer(#[from] WriterError),

    /// Business logic error from assert-entity flow.
    #[error(transparent)]
    AssertEntity(#[from] AssertEntityError),

    /// Branch command error.
    #[error(transparent)]
    Branch(#[from] BranchCommandError),

    /// Branch state error.
    #[error(transparent)]
    BranchState(#[from] BranchStateError),
}
