mod entity_property;
mod entity_type;
pub(crate) mod branch_registry;
pub(crate) mod reserved;
pub(crate) mod slug;

pub use entity_property::{EntityProperty, ValueType};
pub use entity_type::EntityType;
pub use branch_registry::{AddPropertiesInput, EntityTypeInput, PropertyDef, BranchSchemaRegistry};

/// Errors from schema registry operations.
#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    /// Slug failed validation (empty, uppercase, consecutive hyphens, etc.).
    #[error("invalid slug: {0}")]
    InvalidSlug(String),

    /// An entity type with this slug already exists.
    #[error("entity type already exists: {0}")]
    DuplicateEntityType(String),

    /// A property with this slug already exists.
    #[error("property already exists: {0}")]
    DuplicateProperty(String),

    /// Referenced entity type not found.
    #[error("entity type not found: {0}")]
    EntityTypeNotFound(String),

    /// Referenced property not found.
    #[error("property not found: {0}")]
    PropertyNotFound(String),

    /// Attempted to use a reserved property slug (`entity-uuid`, `entity-name`, `entity-type`).
    #[error("reserved property: {0}")]
    ReservedProperty(String),

    /// Error within a batch operation, with the index of the failing item.
    #[error("batch item {index}: {source}")]
    BatchItemError {
        index: usize,
        source: Box<SchemaError>,
    },

    /// Underlying storage error.
    #[error("storage error: {0}")]
    Storage(String),
}
