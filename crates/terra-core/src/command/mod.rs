mod assert_entity;
pub(crate) mod branch;
pub mod branch_state;
pub mod execute;

pub use assert_entity::AssertEntityError;
pub use branch_state::{BranchState, BranchStateError};
pub use execute::execute;
pub use branch::{BranchCommandError, BranchDetail, BranchSummary};

use serde::Serialize;

use crate::assertion::{EntityError, EntityRecord, InvestigationError, LogEntry, LogError, PropertyValue, Transaction, WriterError};
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
    /// Unified write command: schema creation, visibility, entity introduction and assertions.
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

/// Input for creating an entity type with inline property definitions.
pub struct CreateEntityType {
    pub slug: String,
    pub description: Option<String>,
    /// Inline property definitions (slug + value_type + description).
    pub properties: Vec<CreatePropertyDef>,
}

/// Inline property definition for entity type creation.
pub struct CreatePropertyDef {
    pub slug: String,
    pub value_type: ValueType,
    pub description: Option<String>,
}

/// Input for adding properties to an existing entity type.
pub struct AddProperties {
    pub entity_type: String,
    pub properties: Vec<CreatePropertyDef>,
}

/// A single fact or hypothesis: properties + reasoning (entity_type derived from entity).
pub struct AssertionItem {
    /// Property slug → typed value.
    pub properties: HashMap<String, PropertyValue>,
    /// Per-assertion reasoning: why this specific value.
    pub reasoning: serde_json::Value,
}

/// Input for the unified transaction command.
///
/// All write operations are expressed here: schema creation with inline properties,
/// adding properties to existing types, visibility changes, entity introduction, and assertions.
/// Processed in order: entity_types (with inline props) → add_properties → hide/unhide → introduce → asserts.
pub struct TransactionInput {
    /// Transaction-level reasoning: why this batch of operations.
    pub reasoning: serde_json::Value,
    /// The user's question or input that triggered this transaction.
    pub question: Option<String>,
    /// The agent's answer or explanation for this transaction.
    pub answer: Option<String>,
    /// Tool invocations recorded in this transaction (query, reasoning, stats — no result data).
    pub commands: Vec<serde_json::Value>,
    /// Entity types to create with inline property definitions.
    pub entity_types: Vec<CreateEntityType>,
    /// Properties to add to existing entity types.
    pub add_properties: Vec<AddProperties>,
    /// Items to hide on the current branch.
    pub hide: HideUnhideInput,
    /// Items to unhide on the current branch.
    pub unhide: HideUnhideInput,
    /// New entities to introduce (created after schema operations).
    pub introduce: Vec<IntroduceItem>,
    /// Assertions on existing entities (processed after introduces).
    pub asserts: Vec<AssertItem>,
    /// New investigations to create.
    pub investigations: Vec<InvestigationCreateItem>,
    /// Existing investigations to update (notes).
    pub update_investigations: Vec<InvestigationUpdateItem>,
    /// Investigations to close with a resolution.
    pub close_investigations: Vec<InvestigationCloseItem>,
}

/// Items to hide or unhide on a branch, referenced by slug.
#[derive(Default)]
pub struct HideUnhideInput {
    pub entities: Vec<String>,
    pub entity_types: Vec<String>,
    pub properties: Vec<String>,
    pub investigations: Vec<String>,
}

/// A new entity to introduce in a transaction.
pub struct IntroduceItem {
    /// Entity slug.
    pub entity: String,
    /// Entity type slug.
    pub entity_type: String,
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

/// A new investigation to create in a transaction.
pub struct InvestigationCreateItem {
    pub slug: String,
    pub goal: serde_json::Value,
    pub reasoning: String,
    pub context: serde_json::Value,
}

/// An update to an existing investigation's notes.
pub struct InvestigationUpdateItem {
    pub slug: String,
    pub notes: serde_json::Value,
}

/// Close an investigation with a resolution.
pub struct InvestigationCloseItem {
    pub slug: String,
    pub resolution: serde_json::Value,
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

    /// Investigation operation error.
    #[error(transparent)]
    Investigation(#[from] InvestigationError),
}
