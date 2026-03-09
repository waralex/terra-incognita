mod assert_entity;
mod execute;
mod query_entity;

pub use assert_entity::AssertEntityError;
pub use execute::execute;
pub use query_entity::{EntityProjection, ProjectionError, PropertyState};

use crate::assertion::{EntityError, EntityRecord, LogEntry, LogError, PropertyValue, Transaction, WriterError};
use crate::schema::{EntityProperty, EntityType, ValueType};
use crate::schema::SchemaError;

use std::collections::HashMap;

/// Domain command — plain enum without serde. All mutating variants take `Vec`.
pub enum Command {
    /// Create one or more entity types (with optional property attachment).
    CreateEntityTypes(Vec<CreateEntityType>),
    /// List all registered entity types.
    ListEntityTypes,
    /// Get a single entity type with its attached properties.
    GetEntityType { slug: String },
    /// Create one or more properties (with optional entity type attachment).
    CreateProperties(Vec<CreateProperty>),
    /// List properties, optionally filtered by entity type.
    ListProperties { entity_type: Option<String> },
    /// Attach existing properties to existing entity types.
    AttachProperties(Vec<AttachProperty>),
    /// Create entity and optionally assert facts/hypotheses in one transaction.
    CreateEntity(AssertEntityInput),
    /// Assert facts/hypotheses about an existing entity in one transaction.
    AssertEntity(AssertEntityInput),
    /// List all entities (slug + uuid).
    ListEntities,
    /// Get entity projected onto an entity type.
    GetEntity {
        entity: String,
        entity_type: String,
    },
    /// List all entries in the fact log.
    ListLog,
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

/// Unified input for entity creation and assertion.
pub struct AssertEntityInput {
    /// Entity slug.
    pub entity: String,
    /// Entity description (only used during creation).
    pub description: Option<String>,
    /// Transaction-level reasoning: why this batch was made.
    pub reasoning: serde_json::Value,
    /// Facts — convergence points, at most one per property per entity type.
    pub facts: Vec<AssertionItem>,
    /// Hypotheses — tentative claims, no uniqueness constraint on properties.
    pub hypotheses: Vec<AssertionItem>,
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

/// Result of executing a command.
#[derive(Debug)]
pub enum CommandResult {
    /// Created or listed entity types.
    EntityTypes(Vec<EntityType>),
    /// Created or listed properties.
    Properties(Vec<EntityProperty>),
    /// Properties attached; `count` is the number of attachments.
    Attached { count: usize },
    /// Entity created/asserted with transaction and log entries.
    Asserted {
        transaction: Transaction,
        facts: Vec<LogEntry>,
        hypotheses: Vec<LogEntry>,
    },
    /// Single entity type with its attached properties.
    EntityTypeDetail {
        entity_type: EntityType,
        properties: Vec<EntityProperty>,
    },
    /// List of entities (slug + uuid).
    EntityList(Vec<EntityRecord>),
    /// Entity projected onto an entity type.
    EntityDetail(EntityProjection),
    /// Full assertion log.
    LogEntries(Vec<LogEntry>),
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

    /// Projection error from entity query.
    #[error(transparent)]
    Projection(#[from] ProjectionError),
}
