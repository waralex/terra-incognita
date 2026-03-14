//! Top-level storage — opens TerraDb with all known entry types registered.

use std::path::Path;
use std::sync::Arc;

use crate::config::ProjectConfig;
use crate::io::{DbError, TerraDb};
use crate::io::slug::Slug;
use crate::store::branch::Branch;

use crate::store::assertion_entry::AssertionEntry;
use crate::store::entity_change_entry::EntityChangeEntry;
use crate::store::branch_entry::BranchEntry;
use crate::store::entity_entry::EntityEntry;
use crate::store::managed_entry::ManagedEntry;
use crate::store::schema_prop_entry::SchemaPropEntry;
use crate::store::schema_type_entry::SchemaTypeEntry;
use crate::store::slug_entry::SlugEntry;
use crate::store::transaction_entry::TransactionEntry;
use crate::store::visibility_entry::VisibilityEntry;

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

    /// Get the main branch.
    pub fn main_branch(&self) -> Branch {
        Branch::main(self.clone())
    }

    /// Open a branch by slug.
    pub fn branch(&self, branch_id: Slug) -> Result<Branch, DbError> {
        Branch::open(self.clone(), branch_id)
    }

    /// Access the project config.
    pub fn config(&self) -> &ProjectConfig {
        &self.config
    }

    fn open_impl(path: &Path, read_only: bool, config: Arc<ProjectConfig>) -> Result<Self, DbError> {
        let mut builder = TerraDb::builder(path)
            .with::<AssertionEntry>()
            .with::<EntityChangeEntry>()
            .with::<BranchEntry>()
            .with::<EntityEntry>()
            .with::<ManagedEntry>()
            .with::<SchemaPropEntry>()
            .with::<SchemaTypeEntry>()
            .with::<SlugEntry>()
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
    use crate::store::branch::main_branch_slug;

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
