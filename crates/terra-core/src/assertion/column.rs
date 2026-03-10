use std::sync::Arc;

use rocksdb::DB;
use serde::Serialize;
use uuid::Uuid;

use super::key::{storage_key, StorageKey};
use super::log::LogError;

/// A single cell in a column: one property value for one entity at one point in time.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnCell {
    pub branch_id: Uuid,
    pub property_id: Uuid,
    pub tx_id: Uuid,
    pub log_entry_id: Uuid,
    pub entity_id: Uuid,
    pub value: serde_json::Value,
}

/// Low-level columnar storage backed by a RocksDB column family.
///
/// Key layout (72 bytes, big-endian where applicable):
/// `branch_id(16) | property_id(16) | tx_id(8) | log_entry_id(16) | entity_id(16)`
///
/// Value: arbitrary JSON bytes.
pub struct Column {
    db: Arc<DB>,
    cf_name: &'static str,
}

impl Column {
    pub(crate) fn new(db: Arc<DB>, cf_name: &'static str) -> Self {
        Self { db, cf_name }
    }

    /// Writes a single cell.
    pub fn put(&self, cell: &ColumnCell) -> Result<(), LogError> {
        let key = ColumnKey::from_cell(cell).encode();
        let val = serde_json::to_vec(&cell.value)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        let cf = self.cf()?;
        self.db
            .put_cf(cf, &key, &val)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Writes multiple cells atomically via WriteBatch.
    pub fn put_batch(&self, cells: &[ColumnCell]) -> Result<(), LogError> {
        let cf = self.cf()?;
        let mut batch = rocksdb::WriteBatch::default();

        for cell in cells {
            let key = ColumnKey::from_cell(cell).encode();
            let val = serde_json::to_vec(&cell.value)
                .map_err(|e| LogError::Storage(e.to_string()))?;
            batch.put_cf(cf, &key, &val);
        }

        self.db
            .write(batch)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Scans all cells for a given property on the main branch, ordered by timestamp.
    pub fn scan_property(&self, property_id: Uuid) -> Result<Vec<ColumnCell>, LogError> {
        let cf = self.cf()?;
        let prefix = ColumnKey::prefix_branch_property(&super::MAIN_BRANCH, &property_id);

        let mut cells = Vec::new();
        let iter = self.db.prefix_iterator_cf(cf, &prefix);

        for item in iter {
            let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;

            if !raw_key.starts_with(&prefix) {
                break;
            }

            let k = ColumnKey::decode(&raw_key)?;
            let value: serde_json::Value = serde_json::from_slice(&val)
                .map_err(|e| LogError::Storage(e.to_string()))?;

            cells.push(ColumnCell {
                branch_id: k.branch_id,
                property_id: k.property_id,
                tx_id: k.tx_id,
                log_entry_id: k.log_entry_id,
                entity_id: k.entity_id,
                value,
            });
        }

        Ok(cells)
    }

    /// Returns the latest cell for a given property+entity, or None.
    pub fn latest_for_entity(
        &self,
        property_id: Uuid,
        entity_id: Uuid,
    ) -> Result<Option<ColumnCell>, LogError> {
        let cf = self.cf()?;
        let prefix = ColumnKey::prefix_branch_property(&super::MAIN_BRANCH, &property_id);
        let iter = self.db.prefix_iterator_cf(cf, &prefix);

        let mut latest: Option<ColumnCell> = None;
        for item in iter {
            let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !raw_key.starts_with(&prefix) {
                break;
            }
            let k = ColumnKey::decode(&raw_key)?;
            if k.entity_id != entity_id {
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_slice(&val).map_err(|e| LogError::Storage(e.to_string()))?;
            // Keys are sorted by timestamp, so last match wins
            latest = Some(ColumnCell {
                branch_id: k.branch_id,
                property_id: k.property_id,
                tx_id: k.tx_id,
                log_entry_id: k.log_entry_id,
                entity_id: k.entity_id,
                value,
            });
        }
        Ok(latest)
    }

    /// Returns cells for a given property+entity where log_entry_id > the given threshold.
    pub fn list_after(
        &self,
        property_id: Uuid,
        entity_id: Uuid,
        after_log_entry_id: Uuid,
    ) -> Result<Vec<ColumnCell>, LogError> {
        let cf = self.cf()?;
        let prefix = ColumnKey::prefix_branch_property(&super::MAIN_BRANCH, &property_id);
        let iter = self.db.prefix_iterator_cf(cf, &prefix);
        let threshold = *after_log_entry_id.as_bytes();

        let mut cells = Vec::new();
        for item in iter {
            let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !raw_key.starts_with(&prefix) {
                break;
            }
            let k = ColumnKey::decode(&raw_key)?;
            if k.entity_id == entity_id && *k.log_entry_id.as_bytes() > threshold {
                let value: serde_json::Value = serde_json::from_slice(&val)
                    .map_err(|e| LogError::Storage(e.to_string()))?;
                cells.push(ColumnCell {
                    branch_id: k.branch_id,
                    property_id: k.property_id,
                    tx_id: k.tx_id,
                    log_entry_id: k.log_entry_id,
                    entity_id: k.entity_id,
                    value,
                });
            }
        }
        Ok(cells)
    }

    /// Counts cells for a given property+entity where log_entry_id > the given threshold.
    pub fn count_after(
        &self,
        property_id: Uuid,
        entity_id: Uuid,
        after_log_entry_id: Uuid,
    ) -> Result<usize, LogError> {
        let cf = self.cf()?;
        let prefix = ColumnKey::prefix_branch_property(&super::MAIN_BRANCH, &property_id);
        let iter = self.db.prefix_iterator_cf(cf, &prefix);
        let threshold = *after_log_entry_id.as_bytes();

        let mut count = 0;
        for item in iter {
            let (raw_key, _) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !raw_key.starts_with(&prefix) {
                break;
            }
            let k = ColumnKey::decode(&raw_key)?;
            if k.entity_id == entity_id && *k.log_entry_id.as_bytes() > threshold {
                count += 1;
            }
        }
        Ok(count)
    }

    pub(crate) fn cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.cf_name)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.cf_name)))
    }
}

storage_key! {
    pub(crate) struct ColumnKey(80) {
        branch_id: Uuid,
        property_id: Uuid,
        tx_id: Uuid,
        log_entry_id: Uuid,
        entity_id: Uuid,
    }
    prefixes {
        prefix_branch_property(branch_id: Uuid, property_id: Uuid) -> 32,
    }
}

impl ColumnKey {
    pub(crate) fn from_cell(cell: &ColumnCell) -> Self {
        Self {
            branch_id: cell.branch_id,
            property_id: cell.property_id,
            tx_id: cell.tx_id,
            log_entry_id: cell.log_entry_id,
            entity_id: cell.entity_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocksdb::{ColumnFamilyDescriptor, Options, DB};

    const TEST_CF: &str = "test_column";

    fn open_db(dir: &tempfile::TempDir) -> Arc<DB> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let mut cf_opts = Options::default();
        cf_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));
        let cf = ColumnFamilyDescriptor::new(TEST_CF, cf_opts);
        Arc::new(DB::open_cf_descriptors(&opts, dir.path(), vec![cf]).unwrap())
    }

    fn cell(property_id: Uuid, value: serde_json::Value) -> ColumnCell {
        ColumnCell {
            branch_id: Uuid::nil(),
            property_id,
            tx_id: Uuid::now_v7(),
            log_entry_id: Uuid::now_v7(),
            entity_id: Uuid::now_v7(),
            value,
        }
    }

    #[test]
    fn put_and_scan() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let col = Column::new(Arc::clone(&db), TEST_CF);

        let prop = Uuid::now_v7();
        let c = cell(prop, serde_json::json!({"v": 42}));
        col.put(&c).unwrap();

        let cells = col.scan_property(prop).unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].property_id, prop);
        assert_eq!(cells[0].value, serde_json::json!({"v": 42}));
    }

    #[test]
    fn batch_put() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let col = Column::new(Arc::clone(&db), TEST_CF);

        let prop = Uuid::now_v7();
        let cells = vec![
            cell(prop, serde_json::json!("first")),
            cell(prop, serde_json::json!("second")),
            cell(prop, serde_json::json!("third")),
        ];
        col.put_batch(&cells).unwrap();

        let result = col.scan_property(prop).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn scan_isolates_by_property() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let col = Column::new(Arc::clone(&db), TEST_CF);

        let prop_a = Uuid::now_v7();
        let prop_b = Uuid::now_v7();

        col.put(&cell(prop_a, serde_json::json!("a1"))).unwrap();
        col.put(&cell(prop_b, serde_json::json!("b1"))).unwrap();
        col.put(&cell(prop_a, serde_json::json!("a2"))).unwrap();

        let a_cells = col.scan_property(prop_a).unwrap();
        assert_eq!(a_cells.len(), 2);

        let b_cells = col.scan_property(prop_b).unwrap();
        assert_eq!(b_cells.len(), 1);
        assert_eq!(b_cells[0].value, serde_json::json!("b1"));
    }

    #[test]
    fn scan_empty_property() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let col = Column::new(Arc::clone(&db), TEST_CF);

        let cells = col.scan_property(Uuid::now_v7()).unwrap();
        assert!(cells.is_empty());
    }

    #[test]
    fn key_encoding_roundtrip() {
        let c = cell(Uuid::now_v7(), serde_json::json!(null));
        let key = ColumnKey::from_cell(&c);
        let encoded = key.encode();
        assert_eq!(encoded.len(), ColumnKey::SIZE);

        let decoded = ColumnKey::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn keys_sort_by_branch_property_tx() {
        let branch = Uuid::nil();
        let prop = Uuid::now_v7();
        let lid = Uuid::now_v7();
        let eid = Uuid::now_v7();
        let tx1 = Uuid::from_u128(100);
        let tx2 = Uuid::from_u128(200);

        let k1 = ColumnKey { branch_id: branch, property_id: prop, tx_id: tx1, log_entry_id: lid, entity_id: eid };
        let k2 = ColumnKey { branch_id: branch, property_id: prop, tx_id: tx2, log_entry_id: lid, entity_id: eid };

        assert!(k1.encode() < k2.encode());
    }

    #[test]
    fn list_after_matches_count_after() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let col = Column::new(Arc::clone(&db), TEST_CF);

        let prop = Uuid::now_v7();
        let entity = Uuid::now_v7();

        // Create a threshold cell
        let threshold_cell = ColumnCell {
            branch_id: Uuid::nil(),
            property_id: prop,
            tx_id: Uuid::now_v7(),
            log_entry_id: Uuid::now_v7(),
            entity_id: entity,
            value: serde_json::json!("before"),
        };
        col.put(&threshold_cell).unwrap();

        // Create cells after the threshold
        let after1 = ColumnCell {
            branch_id: Uuid::nil(),
            property_id: prop,
            tx_id: Uuid::now_v7(),
            log_entry_id: Uuid::now_v7(),
            entity_id: entity,
            value: serde_json::json!("after-1"),
        };
        let after2 = ColumnCell {
            branch_id: Uuid::nil(),
            property_id: prop,
            tx_id: Uuid::now_v7(),
            log_entry_id: Uuid::now_v7(),
            entity_id: entity,
            value: serde_json::json!("after-2"),
        };
        col.put(&after1).unwrap();
        col.put(&after2).unwrap();

        let count = col.count_after(prop, entity, threshold_cell.log_entry_id).unwrap();
        let cells = col.list_after(prop, entity, threshold_cell.log_entry_id).unwrap();

        assert_eq!(count, 2);
        assert_eq!(cells.len(), count);
        assert_eq!(cells[0].value, serde_json::json!("after-1"));
        assert_eq!(cells[1].value, serde_json::json!("after-2"));
    }
}
