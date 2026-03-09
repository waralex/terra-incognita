pub(crate) mod column;
mod entity;
pub(crate) mod entity_io;
pub(crate) mod log;
mod property_value;
mod store;
pub(crate) mod writer;

/// The default branch UUID (all zeros). All operations use this until branching is implemented.
pub const MAIN_BRANCH: uuid::Uuid = uuid::Uuid::nil();

pub use column::{Column, ColumnCell};
pub use entity::EntityStore;
pub use entity_io::{EntityRecord, EntityStatus};
pub use log::{AppendLog, LogEntry, LogError};
pub use property_value::{PropertyValue, RangeValue, SetValue, StructValue};
pub use store::AssertionStore;
pub use writer::{AssertionInput, AssertionWriter, WriterError};
pub use entity::EntityError;
