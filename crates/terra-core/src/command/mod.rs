mod execute;

pub use execute::execute;

use crate::assertion::AssertionError;
use crate::schema::{EntityProperty, EntityType, ValueType};
use crate::assertion::LogEntry;
use crate::schema::SchemaError;

pub enum Command {
    CreateEntityTypes(Vec<CreateEntityType>),
    ListEntityTypes,
    GetEntityType { slug: String },
    CreateProperties(Vec<CreateProperty>),
    ListProperties { entity_type: Option<String> },
    AttachProperties(Vec<AttachProperty>),
    CreateEntities(Vec<CreateEntity>),
    ListLog,
}

pub struct CreateEntityType {
    pub slug: String,
    pub description: Option<String>,
    pub properties: Vec<String>,
}

pub struct CreateProperty {
    pub slug: String,
    pub value_type: ValueType,
    pub description: Option<String>,
    pub entity_types: Vec<String>,
}

pub struct AttachProperty {
    pub entity_type: String,
    pub property: String,
}

pub struct CreateEntity {
    pub entity_name: String,
    pub entity_type: Option<String>,
    pub context: serde_json::Value,
}

#[derive(Debug)]
pub enum CommandResult {
    EntityTypes(Vec<EntityType>),
    Properties(Vec<EntityProperty>),
    Attached { count: usize },
    Entities(Vec<LogEntry>),
    EntityTypeDetail {
        entity_type: EntityType,
        properties: Vec<EntityProperty>,
    },
    LogEntries(Vec<LogEntry>),
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Schema(#[from] SchemaError),

    #[error(transparent)]
    Assertion(#[from] AssertionError),
}
