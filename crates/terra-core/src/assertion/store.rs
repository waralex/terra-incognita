use std::path::Path;
use std::sync::Arc;

use rocksdb::{ColumnFamilyDescriptor, Options, DB};

use super::column::Column;
use super::entity::EntityStore;
use super::entity_io::EntityIo;
use super::log::AppendLog;
use super::writer::AssertionWriter;
use super::LogError;

const CF_FACTS: &str = "facts";
const CF_HYPOTHESES: &str = "hypotheses";
const CF_FACT_SET: &str = "fact_set";
const CF_FACT_STRUCT: &str = "fact_struct";
const CF_FACT_RANGE: &str = "fact_range";
const CF_HYP_SET: &str = "hyp_set";
const CF_HYP_STRUCT: &str = "hyp_struct";
const CF_HYP_RANGE: &str = "hyp_range";
const CF_ENTITY_MAIN: &str = "entity_main";
const CF_ENTITY_SLUG: &str = "entity_slug";

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

        let mut col_opts = Options::default();
        col_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));

        let mut entity_opts = Options::default();
        entity_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_FACTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_HYPOTHESES, Options::default()),
            ColumnFamilyDescriptor::new(CF_FACT_SET, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_FACT_STRUCT, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_FACT_RANGE, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_SET, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_STRUCT, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_RANGE, col_opts),
            ColumnFamilyDescriptor::new(CF_ENTITY_MAIN, entity_opts),
            ColumnFamilyDescriptor::new(CF_ENTITY_SLUG, Options::default()),
        ];
        let db = DB::open_cf_descriptors(&opts, path, cfs)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        Ok(Self { db: Arc::new(db) })
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
        AssertionWriter::new(Arc::clone(&self.db), CF_FACTS, CF_FACT_SET, CF_FACT_STRUCT, CF_FACT_RANGE)
    }

    /// Writer for hypothesis assertions.
    pub fn hypothesis_writer(&self) -> AssertionWriter {
        AssertionWriter::new(Arc::clone(&self.db), CF_HYPOTHESES, CF_HYP_SET, CF_HYP_STRUCT, CF_HYP_RANGE)
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

    // -- Entities --

    /// Entity store for create/delete/restore/find operations.
    pub fn entities(&self) -> EntityStore {
        EntityStore::new(EntityIo::new(Arc::clone(&self.db), CF_ENTITY_MAIN, CF_ENTITY_SLUG))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

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
            .append(eid1, serde_json::json!({"name": "alpha"}))
            .unwrap();
        s.hypotheses()
            .append(eid2, serde_json::json!({"name": "beta"}))
            .unwrap();

        let facts = s.facts().list().unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].body["name"], "alpha");

        let hyps = s.hypotheses().list().unwrap();
        assert_eq!(hyps.len(), 1);
        assert_eq!(hyps[0].body["name"], "beta");
    }

    #[test]
    fn batch_into_separate_logs() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let fact_items: Vec<(Uuid, serde_json::Value)> = vec![
            (Uuid::now_v7(), serde_json::json!({"name": "r1"})),
            (Uuid::now_v7(), serde_json::json!({"name": "r2"})),
        ];
        s.facts().append_batch(&fact_items).unwrap();

        let hyp_items: Vec<(Uuid, serde_json::Value)> = vec![
            (Uuid::now_v7(), serde_json::json!({"name": "h1"})),
        ];
        s.hypotheses().append_batch(&hyp_items).unwrap();

        assert_eq!(s.facts().list().unwrap().len(), 2);
        assert_eq!(s.hypotheses().list().unwrap().len(), 1);
    }
}
