mod entity_property;
mod entity_type;
mod migrations;
pub(crate) mod reserved;
mod registry;
pub(crate) mod slug;

pub use entity_property::{EntityProperty, ValueType};
pub use entity_type::EntityType;
pub use registry::{AttachInput, EntityTypeInput, PropertyInput, SchemaRegistry};

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("invalid slug: {0}")]
    InvalidSlug(String),

    #[error("entity type already exists: {0}")]
    DuplicateEntityType(String),

    #[error("property already exists: {0}")]
    DuplicateProperty(String),

    #[error("entity type not found: {0}")]
    EntityTypeNotFound(String),

    #[error("property not found: {0}")]
    PropertyNotFound(String),

    #[error("reserved property: {0}")]
    ReservedProperty(String),

    #[error("batch item {index}: {source}")]
    BatchItemError {
        index: usize,
        source: Box<SchemaError>,
    },

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
}
