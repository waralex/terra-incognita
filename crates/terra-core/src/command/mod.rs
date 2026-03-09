mod execute;

pub use execute::execute;

use crate::assertion::{LogEntry, LogError};
use crate::schema::{EntityProperty, EntityType, ValueType};
use crate::schema::SchemaError;

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
    /// Create one or more entities in the refinement log.
    CreateEntities(Vec<CreateEntity>),
    /// List all entries in the refinement log.
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

/// Input for creating an entity in the assertion log.
pub struct CreateEntity {
    pub entity_name: String,
    pub entity_type: Option<String>,
    pub context: serde_json::Value,
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
    /// Created entities (log entries).
    Entities(Vec<LogEntry>),
    /// Single entity type with its attached properties.
    EntityTypeDetail {
        entity_type: EntityType,
        properties: Vec<EntityProperty>,
    },
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
}
