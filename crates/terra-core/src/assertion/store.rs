use std::path::Path;
use std::sync::Arc;

use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use uuid::Uuid;

use super::branch::BranchStore;
use super::branch_io::BranchIo;
use super::column::Column;
use super::entity::EntityStore;
use super::entity_io::EntityIo;
use super::investigation::InvestigationStore;
use super::investigation_io::InvestigationIo;
use super::log::AppendLog;
use super::transaction::TransactionStore;
use super::writer::AssertionWriter;
use super::LogError;
use crate::schema::BranchSchemaRegistry;

const CF_FACTS: &str = "facts";
const CF_HYPOTHESES: &str = "hypotheses";
const CF_FACT_SET: &str = "fact_set";
const CF_FACT_STRUCT: &str = "fact_struct";
const CF_FACT_RANGE: &str = "fact_range";
const CF_HYP_SET: &str = "hyp_set";
const CF_HYP_STRUCT: &str = "hyp_struct";
const CF_HYP_RANGE: &str = "hyp_range";
const CF_TRANSACTIONS: &str = "transactions";
const CF_ENTITY_MAIN: &str = "entity_main";
const CF_ENTITY_SLUG: &str = "entity_slug";
const CF_BRANCH_MAIN: &str = "branch_main";
const CF_BRANCH_SLUG: &str = "branch_slug";
const CF_SCHEMA_TYPES: &str = "schema_types";
const CF_SCHEMA_TYPE_SLUG: &str = "schema_type_slug";
const CF_SCHEMA_PROPS: &str = "schema_props";
const CF_SCHEMA_PROP_SLUG: &str = "schema_prop_slug";
const CF_SCHEMA_ATTACHMENTS: &str = "schema_attachments";
const CF_VISIBILITY: &str = "visibility";
const CF_INVESTIGATION_MAIN: &str = "investigation_main";
const CF_INVESTIGATION_SLUG: &str = "investigation_slug";

/// RocksDB-backed store owning logs and typed columns for facts and hypotheses.
pub struct AssertionStore {
    db: Arc<DB>,
}

impl AssertionStore {
    /// Opens or creates the store at the given path with all column families.
    pub fn open(path: &Path) -> Result<Self, LogError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let cfs = Self::column_families();
        let db = DB::open_cf_descriptors(&opts, path, cfs)
            .map_err(|e| LogError::Storage(e.to_string()))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Opens the store in read-only mode (allows concurrent access with a writer).
    pub fn open_read_only(path: &Path) -> Result<Self, LogError> {
        let opts = Options::default();
        let cfs = Self::column_families();
        let db = DB::open_cf_descriptors_read_only(&opts, path, cfs, false)
            .map_err(|e| LogError::Storage(e.to_string()))?;
        Ok(Self { db: Arc::new(db) })
    }

    fn column_families() -> Vec<ColumnFamilyDescriptor> {
        let mut col_opts = Options::default();
        col_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(32));

        let mut entity_opts = Options::default();
        entity_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));

        let mut schema_type_opts = Options::default();
        schema_type_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));

        let mut schema_attach_opts = Options::default();
        schema_attach_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(32));

        vec![
            ColumnFamilyDescriptor::new(CF_FACTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_HYPOTHESES, Options::default()),
            ColumnFamilyDescriptor::new(CF_FACT_SET, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_FACT_STRUCT, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_FACT_RANGE, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_SET, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_STRUCT, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_RANGE, col_opts),
            ColumnFamilyDescriptor::new(CF_TRANSACTIONS, Options::default()),
            ColumnFamilyDescriptor::new(CF_ENTITY_MAIN, entity_opts.clone()),
            ColumnFamilyDescriptor::new(CF_ENTITY_SLUG, Options::default()),
            ColumnFamilyDescriptor::new(CF_BRANCH_MAIN, Options::default()),
            ColumnFamilyDescriptor::new(CF_BRANCH_SLUG, Options::default()),
            ColumnFamilyDescriptor::new(CF_SCHEMA_TYPES, schema_type_opts.clone()),
            ColumnFamilyDescriptor::new(CF_SCHEMA_TYPE_SLUG, entity_opts.clone()),
            ColumnFamilyDescriptor::new(CF_SCHEMA_PROPS, schema_type_opts),
            ColumnFamilyDescriptor::new(CF_SCHEMA_PROP_SLUG, entity_opts.clone()),
            ColumnFamilyDescriptor::new(CF_SCHEMA_ATTACHMENTS, schema_attach_opts),
            ColumnFamilyDescriptor::new(CF_VISIBILITY, entity_opts.clone()),
            ColumnFamilyDescriptor::new(CF_INVESTIGATION_MAIN, entity_opts.clone()),
            ColumnFamilyDescriptor::new(CF_INVESTIGATION_SLUG, entity_opts),
        ]
    }

    // -- Logs --

    /// Fact log — convergence points, definitive claims.
    pub fn facts(&self) -> AppendLog {
        AppendLog::new(Arc::clone(&self.db), CF_FACTS)
    }

    /// Hypothesis log — tentative claims under consideration.
    pub fn hypotheses(&self) -> AppendLog {
        AppendLog::new(Arc::clone(&self.db), CF_HYPOTHESES)
    }

    // -- Writers (log + columns in one WriteBatch) --

    /// Writer for fact assertions.
    pub fn fact_writer(&self) -> AssertionWriter {
        AssertionWriter::new(Arc::clone(&self.db), CF_FACTS, CF_TRANSACTIONS, CF_FACT_SET, CF_FACT_STRUCT, CF_FACT_RANGE)
    }

    /// Writer for hypothesis assertions.
    pub fn hypothesis_writer(&self) -> AssertionWriter {
        AssertionWriter::new(Arc::clone(&self.db), CF_HYPOTHESES, CF_TRANSACTIONS, CF_HYP_SET, CF_HYP_STRUCT, CF_HYP_RANGE)
    }

    // -- Column accessors (for reads) --

    /// Fact set column.
    pub fn fact_col_set(&self) -> Column {
        Column::new(Arc::clone(&self.db), CF_FACT_SET)
    }

    /// Fact struct column.
    pub fn fact_col_struct(&self) -> Column {
        Column::new(Arc::clone(&self.db), CF_FACT_STRUCT)
    }

    /// Fact range column.
    pub fn fact_col_range(&self) -> Column {
        Column::new(Arc::clone(&self.db), CF_FACT_RANGE)
    }

    /// Hypothesis set column.
    pub fn hypothesis_col_set(&self) -> Column {
        Column::new(Arc::clone(&self.db), CF_HYP_SET)
    }

    /// Hypothesis struct column.
    pub fn hypothesis_col_struct(&self) -> Column {
        Column::new(Arc::clone(&self.db), CF_HYP_STRUCT)
    }

    /// Hypothesis range column.
    pub fn hypothesis_col_range(&self) -> Column {
        Column::new(Arc::clone(&self.db), CF_HYP_RANGE)
    }

    // -- Transactions --

    /// Transaction store for grouping related assertions.
    pub fn transactions(&self) -> TransactionStore {
        TransactionStore::new(Arc::clone(&self.db), CF_TRANSACTIONS)
    }

    /// Commits a pre-built WriteBatch atomically.
    pub(crate) fn write_batch(&self, batch: rocksdb::WriteBatch) -> Result<(), LogError> {
        self.db
            .write(batch)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    // -- Entities --

    /// Entity store for create/delete/restore/find operations (branch-aware).
    pub fn entities(&self, branch_id: Uuid, ancestry: Vec<(Uuid, Uuid)>) -> EntityStore {
        EntityStore::new(EntityIo::new(Arc::clone(&self.db), CF_ENTITY_MAIN, CF_ENTITY_SLUG, branch_id, ancestry))
    }

    // -- Investigations --

    /// Investigation store for create/update/close/list operations (branch-aware).
    pub fn investigations(&self, branch_id: Uuid, ancestry: Vec<(Uuid, Uuid)>) -> InvestigationStore {
        InvestigationStore::new(InvestigationIo::new(Arc::clone(&self.db), CF_INVESTIGATION_MAIN, CF_INVESTIGATION_SLUG, branch_id, ancestry))
    }

    // -- Branches --

    /// Branch store for creating and managing branches.
    pub fn branches(&self) -> BranchStore {
        BranchStore::new(BranchIo::new(Arc::clone(&self.db), CF_BRANCH_MAIN, CF_BRANCH_SLUG))
    }

    // -- Visibility --

    /// Visibility store for hide/unhide operations.
    pub fn visibility(&self) -> super::visibility::VisibilityStore {
        super::visibility::VisibilityStore::new(Arc::clone(&self.db), CF_VISIBILITY)
    }

    // -- Schema --

    /// Creates a branch-scoped schema registry.
    pub fn schema_registry(&self, branch_id: Uuid, ancestry: Vec<(Uuid, Uuid)>) -> BranchSchemaRegistry {
        BranchSchemaRegistry::new(Arc::clone(&self.db), branch_id, ancestry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> AssertionStore {
        AssertionStore::open(dir.path()).unwrap()
    }

    #[test]
    fn facts_and_hypotheses_are_separate() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let eid1 = Uuid::now_v7();
        let eid2 = Uuid::now_v7();

        s.facts()
            .append(Uuid::now_v7(), eid1, serde_json::json!({"name": "alpha"}), serde_json::json!(null))
            .unwrap();
        s.hypotheses()
            .append(Uuid::now_v7(), eid2, serde_json::json!({"name": "beta"}), serde_json::json!(null))
            .unwrap();

        let facts = s.facts().list().unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].properties["name"], "alpha");

        let hyps = s.hypotheses().list().unwrap();
        assert_eq!(hyps.len(), 1);
        assert_eq!(hyps[0].properties["name"], "beta");
    }

    #[test]
    fn batch_into_separate_logs() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let fact_items: Vec<(Uuid, serde_json::Value, serde_json::Value)> = vec![
            (Uuid::now_v7(), serde_json::json!({"name": "r1"}), serde_json::json!(null)),
            (Uuid::now_v7(), serde_json::json!({"name": "r2"}), serde_json::json!(null)),
        ];
        s.facts().append_batch(Uuid::now_v7(), &fact_items).unwrap();

        let hyp_items: Vec<(Uuid, serde_json::Value, serde_json::Value)> = vec![
            (Uuid::now_v7(), serde_json::json!({"name": "h1"}), serde_json::json!(null)),
        ];
        s.hypotheses().append_batch(Uuid::now_v7(), &hyp_items).unwrap();

        assert_eq!(s.facts().list().unwrap().len(), 2);
        assert_eq!(s.hypotheses().list().unwrap().len(), 1);
    }
}
