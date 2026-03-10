pub(crate) mod branch;
pub(crate) mod branch_io;
pub(crate) mod column;
mod entity;
pub(crate) mod entity_io;
pub(crate) mod key;
pub(crate) mod log;
mod property_value;
mod store;
pub(crate) mod transaction;
pub(crate) mod visibility;
pub(crate) mod writer;

/// The default branch UUID (all zeros). The main branch.
pub const MAIN_BRANCH: uuid::Uuid = uuid::Uuid::nil();

pub use branch::BranchError;
pub use branch::BranchStore;
pub use branch_io::BranchRecord;
pub use column::{Column, ColumnCell};
pub use entity::EntityStore;
pub use entity_io::{EntityRecord, EntityStatus};
pub use log::{AppendLog, LogEntry, LogError};
pub use property_value::{PropertyValue, RangeValue, SetValue, StructValue};
pub use store::AssertionStore;
pub use transaction::{Transaction, TransactionStore};
pub use visibility::{ItemKind, VisibilityStore};
pub use writer::{AssertionInput, AssertionWriter, WriterError};
pub use entity::EntityError;
