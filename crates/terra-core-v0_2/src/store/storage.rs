//! Top-level storage — opens TerraDb with all known entry types registered.

use std::path::Path;
use std::sync::Arc;

use crate::config::ProjectConfig;
use crate::io::{DbError, DbItem, TerraDb, WriteBatch};
use crate::io::slug::Slug;
use crate::store::branch_context::BranchContext;

use crate::store::entry::assertion::AssertionEntry;
use crate::store::entry::branch::BranchEntry;
use crate::store::entry::entity::EntityEntry;
use crate::store::entry::entity_change::EntityChangeEntry;
use crate::store::entry::managed::ManagedEntry;
use crate::store::entry::transaction::TransactionEntry;
use crate::store::entry::visibility::VisibilityEntry;

/// Top-level storage. Owns the database and project config.
#[derive(Clone)]
pub struct Storage {
    pub(crate) db: TerraDb,
    config: Arc<ProjectConfig>,
}

impl Storage {
    /// Open storage in read-write mode.
    pub fn open(path: &Path, config: Arc<ProjectConfig>) -> Result<Self, DbError> {
        Self::open_impl(path, false, config)
    }

    /// Open storage in read-only mode.
    pub fn open_read_only(path: &Path, config: Arc<ProjectConfig>) -> Result<Self, DbError> {
        Self::open_impl(path, true, config)
    }

    /// Get the main branch context.
    pub fn main_branch(&self) -> BranchContext {
        BranchContext::main(self.clone())
    }

    /// Open a branch context by slug.
    pub fn branch(&self, branch: Slug) -> Result<BranchContext, DbError> {
        BranchContext::open(self.clone(), branch)
    }

    /// Access the project config.
    pub fn config(&self) -> &ProjectConfig {
        &self.config
    }

    /// Check if any record exists within the given scan range.
    pub fn exists<T: DbItem>(
        &self,
        prefix: &impl crate::io::KeyPrefix<Key = T::Key>,
    ) -> Result<bool, DbError> {
        let mut iter = self.db.scan_rev::<T>(prefix)?;
        Ok(iter.next().is_some())
    }

    /// Get the latest version of a record within the given scan range.
    pub fn get_latest<T: DbItem>(
        &self,
        prefix: &impl crate::io::KeyPrefix<Key = T::Key>,
    ) -> Result<Option<T>, DbError> {
        let mut iter = self.db.scan_rev::<T>(prefix)?;
        match iter.next() {
            Some(result) => Ok(Some(result?)),
            None => Ok(None),
        }
    }

    /// Forward scan over items within the given range.
    pub fn scan<'a, T: DbItem>(
        &'a self,
        prefix: &impl crate::io::KeyPrefix<Key = T::Key>,
    ) -> Result<crate::io::DbIterator<'a, T>, DbError> {
        self.db.scan::<T>(prefix)
    }

    /// Get an item by its exact typed key.
    pub fn get<T: DbItem>(&self, key: &T::Key) -> Result<Option<T>, DbError> {
        self.db.get::<T>(key)
    }

    /// Create a new write batch bound to this database.
    pub fn batch(&self) -> WriteBatch {
        self.db.batch()
    }

    fn open_impl(path: &Path, read_only: bool, config: Arc<ProjectConfig>) -> Result<Self, DbError> {
        let mut builder = TerraDb::builder(path)
            .with::<AssertionEntry>()
            .with::<EntityChangeEntry>()
            .with::<BranchEntry>()
            .with::<EntityEntry>()
            .with::<ManagedEntry>()
            .with::<TransactionEntry>()
            .with::<VisibilityEntry>();
        if read_only {
            builder = builder.read_only();
        }
        Ok(Self { db: builder.open()?, config })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::branch_context::main_branch_slug;

    fn test_config() -> Arc<ProjectConfig> {
        Arc::new(ProjectConfig::builder()
            .data_dir("./data".into())
            .schema_path("./schema.yaml".into())
            .build())
    }

    #[test]
    fn open_and_reopen_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config();
        {
            let _storage = Storage::open(dir.path(), config.clone()).unwrap();
        }
        let storage = Storage::open_read_only(dir.path(), config).unwrap();
        assert_eq!(storage.db.mode(), crate::io::AccessMode::ReadOnly);
    }

    #[test]
    fn main_branch_from_storage() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        assert_eq!(branch.id(), &main_branch_slug());
    }
}
