use std::path::Path;

use rocksdb::{ColumnFamilyDescriptor, Options, DB};

use super::column::Column;
use super::log::AppendLog;
use super::writer::AssertionWriter;
use super::LogError;

const CF_REFINEMENTS: &str = "assertions";
const CF_HYPOTHESES: &str = "hypotheses";
const CF_REF_SET: &str = "ref_set";
const CF_REF_STRUCT: &str = "ref_struct";
const CF_REF_RANGE: &str = "ref_range";
const CF_HYP_SET: &str = "hyp_set";
const CF_HYP_STRUCT: &str = "hyp_struct";
const CF_HYP_RANGE: &str = "hyp_range";

/// RocksDB-backed store owning logs and typed columns for refinements and hypotheses.
pub struct AssertionStore {
    db: DB,
}

impl AssertionStore {
    /// Opens or creates the store at the given path with all column families.
    pub fn open(path: &Path) -> Result<Self, LogError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let mut col_opts = Options::default();
        col_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_REFINEMENTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_HYPOTHESES, Options::default()),
            ColumnFamilyDescriptor::new(CF_REF_SET, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_REF_STRUCT, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_REF_RANGE, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_SET, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_STRUCT, col_opts.clone()),
            ColumnFamilyDescriptor::new(CF_HYP_RANGE, col_opts),
        ];
        let db = DB::open_cf_descriptors(&opts, path, cfs)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        Ok(Self { db })
    }

    // -- Logs --

    /// Refinement log — convergence points, definitive claims.
    pub fn refinements(&self) -> AppendLog<'_> {
        AppendLog::new(&self.db, CF_REFINEMENTS)
    }

    /// Hypothesis log — tentative claims under consideration.
    pub fn hypotheses(&self) -> AppendLog<'_> {
        AppendLog::new(&self.db, CF_HYPOTHESES)
    }

    // -- Writers (log + columns in one WriteBatch) --

    /// Writer for refinement assertions.
    pub fn refinement_writer(&self) -> AssertionWriter<'_> {
        AssertionWriter::new(&self.db, CF_REFINEMENTS, CF_REF_SET, CF_REF_STRUCT, CF_REF_RANGE)
    }

    /// Writer for hypothesis assertions.
    pub fn hypothesis_writer(&self) -> AssertionWriter<'_> {
        AssertionWriter::new(&self.db, CF_HYPOTHESES, CF_HYP_SET, CF_HYP_STRUCT, CF_HYP_RANGE)
    }

    // -- Column accessors (for reads) --

    /// Refinement set column.
    pub fn refinement_col_set(&self) -> Column<'_> {
        Column::new(&self.db, CF_REF_SET)
    }

    /// Refinement struct column.
    pub fn refinement_col_struct(&self) -> Column<'_> {
        Column::new(&self.db, CF_REF_STRUCT)
    }

    /// Refinement range column.
    pub fn refinement_col_range(&self) -> Column<'_> {
        Column::new(&self.db, CF_REF_RANGE)
    }

    /// Hypothesis set column.
    pub fn hypothesis_col_set(&self) -> Column<'_> {
        Column::new(&self.db, CF_HYP_SET)
    }

    /// Hypothesis struct column.
    pub fn hypothesis_col_struct(&self) -> Column<'_> {
        Column::new(&self.db, CF_HYP_STRUCT)
    }

    /// Hypothesis range column.
    pub fn hypothesis_col_range(&self) -> Column<'_> {
        Column::new(&self.db, CF_HYP_RANGE)
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
    fn refinements_and_hypotheses_are_separate() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let eid1 = Uuid::now_v7();
        let eid2 = Uuid::now_v7();

        s.refinements()
            .append(eid1, serde_json::json!({"name": "alpha"}))
            .unwrap();
        s.hypotheses()
            .append(eid2, serde_json::json!({"name": "beta"}))
            .unwrap();

        let refs = s.refinements().list().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].body["name"], "alpha");

        let hyps = s.hypotheses().list().unwrap();
        assert_eq!(hyps.len(), 1);
        assert_eq!(hyps[0].body["name"], "beta");
    }

    #[test]
    fn batch_into_separate_logs() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let ref_items: Vec<(Uuid, serde_json::Value)> = vec![
            (Uuid::now_v7(), serde_json::json!({"name": "r1"})),
            (Uuid::now_v7(), serde_json::json!({"name": "r2"})),
        ];
        s.refinements().append_batch(&ref_items).unwrap();

        let hyp_items: Vec<(Uuid, serde_json::Value)> = vec![
            (Uuid::now_v7(), serde_json::json!({"name": "h1"})),
        ];
        s.hypotheses().append_batch(&hyp_items).unwrap();

        assert_eq!(s.refinements().list().unwrap().len(), 2);
        assert_eq!(s.hypotheses().list().unwrap().len(), 1);
    }
}
