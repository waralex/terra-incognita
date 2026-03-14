//! Top-level storage — opens TerraDb with all known entry types registered.

use std::path::Path;

use crate::io::{DbError, TerraDb};

use crate::store::assertion_entry::AssertionEntry;
use crate::store::entity_change_entry::EntityChangeEntry;
use crate::store::branch_entry::BranchEntry;
use crate::store::entity_entry::EntityEntry;
use crate::store::managed_entry::ManagedEntry;
use crate::store::schema_attachment_entry::SchemaAttachmentEntry;
use crate::store::schema_prop_entry::SchemaPropEntry;
use crate::store::schema_type_entry::SchemaTypeEntry;
use crate::store::slug_entry::SlugEntry;
use crate::store::transaction_entry::TransactionEntry;
use crate::store::visibility_entry::VisibilityEntry;

/// Top-level storage. Owns the database with all column families registered.
pub struct Storage {
    db: TerraDb,
}

impl Storage {
    /// Open storage in read-write mode.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        Self::open_impl(path, false)
    }

    /// Open storage in read-only mode.
    pub fn open_read_only(path: &Path) -> Result<Self, DbError> {
        Self::open_impl(path, true)
    }

    fn open_impl(path: &Path, read_only: bool) -> Result<Self, DbError> {
        let mut builder = TerraDb::builder(path)
            .with::<AssertionEntry>()
            .with::<EntityChangeEntry>()
            .with::<BranchEntry>()
            .with::<EntityEntry>()
            .with::<ManagedEntry>()
            .with::<SchemaAttachmentEntry>()
            .with::<SchemaPropEntry>()
            .with::<SchemaTypeEntry>()
            .with::<SlugEntry>()
            .with::<TransactionEntry>()
            .with::<VisibilityEntry>();
        if read_only {
            builder = builder.read_only();
        }
        Ok(Self { db: builder.open()? })
    }

    /// Access the underlying database.
    pub fn db(&self) -> &TerraDb {
        &self.db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_reopen_read_only() {
        let dir = tempfile::tempdir().unwrap();
        {
            let _storage = Storage::open(dir.path()).unwrap();
        }
        let storage = Storage::open_read_only(dir.path()).unwrap();
        assert_eq!(storage.db().mode(), crate::io::AccessMode::ReadOnly);
    }
}
