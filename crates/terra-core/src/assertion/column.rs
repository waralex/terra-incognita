use std::sync::Arc;

use rocksdb::DB;
use serde::Serialize;
use uuid::Uuid;

use super::log::LogError;

/// A single cell in a column: one property value for one entity at one point in time.
#[derive(Debug, Clone, Serialize)]
pub struct ColumnCell {
    pub property_id: Uuid,
    pub timestamp_us: i64,
    pub log_entry_id: Uuid,
    pub entity_id: Uuid,
    pub value: serde_json::Value,
}

/// Low-level columnar storage backed by a RocksDB column family.
///
/// Key layout (56 bytes, big-endian where applicable):
/// `property_id(16) | timestamp_us(8) | log_entry_id(16) | entity_id(16)`
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
        let key = encode_key(cell);
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
            let key = encode_key(cell);
            let val = serde_json::to_vec(&cell.value)
                .map_err(|e| LogError::Storage(e.to_string()))?;
            batch.put_cf(cf, &key, &val);
        }

        self.db
            .write(batch)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Scans all cells for a given property, ordered by timestamp.
    pub fn scan_property(&self, property_id: Uuid) -> Result<Vec<ColumnCell>, LogError> {
        let cf = self.cf()?;
        let prefix = property_id.as_bytes().to_vec();

        let mut cells = Vec::new();
        let iter = self.db.prefix_iterator_cf(cf, &prefix);

        for item in iter {
            let (key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;

            if !key.starts_with(&prefix) {
                break;
            }

            let (pid, ts, lid, eid) = decode_key(&key)?;
            let value: serde_json::Value = serde_json::from_slice(&val)
                .map_err(|e| LogError::Storage(e.to_string()))?;

            cells.push(ColumnCell {
                property_id: pid,
                timestamp_us: ts,
                log_entry_id: lid,
                entity_id: eid,
                value,
            });
        }

        Ok(cells)
    }

    pub(crate) fn cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.cf_name)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.cf_name)))
    }
}

// Key: property_id(16) | timestamp_us(8 BE) | log_entry_id(16) | entity_id(16) = 56 bytes

pub(crate) fn encode_key(cell: &ColumnCell) -> [u8; 56] {
    let mut key = [0u8; 56];
    key[0..16].copy_from_slice(cell.property_id.as_bytes());
    key[16..24].copy_from_slice(&cell.timestamp_us.to_be_bytes());
    key[24..40].copy_from_slice(cell.log_entry_id.as_bytes());
    key[40..56].copy_from_slice(cell.entity_id.as_bytes());
    key
}

fn decode_key(key: &[u8]) -> Result<(Uuid, i64, Uuid, Uuid), LogError> {
    if key.len() < 56 {
        return Err(LogError::Storage("column key too short".into()));
    }
    let property_id =
        Uuid::from_slice(&key[0..16]).map_err(|e| LogError::Storage(e.to_string()))?;
    let timestamp_us = i64::from_be_bytes(
        key[16..24]
            .try_into()
            .map_err(|_| LogError::Storage("bad timestamp".into()))?,
    );
    let log_entry_id =
        Uuid::from_slice(&key[24..40]).map_err(|e| LogError::Storage(e.to_string()))?;
    let entity_id =
        Uuid::from_slice(&key[40..56]).map_err(|e| LogError::Storage(e.to_string()))?;
    Ok((property_id, timestamp_us, log_entry_id, entity_id))
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

    fn cell(property_id: Uuid, ts: i64, value: serde_json::Value) -> ColumnCell {
        ColumnCell {
            property_id,
            timestamp_us: ts,
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
        let c = cell(prop, 1000, serde_json::json!({"v": 42}));
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
            cell(prop, 100, serde_json::json!("first")),
            cell(prop, 200, serde_json::json!("second")),
            cell(prop, 300, serde_json::json!("third")),
        ];
        col.put_batch(&cells).unwrap();

        let result = col.scan_property(prop).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].timestamp_us, 100);
        assert_eq!(result[1].timestamp_us, 200);
        assert_eq!(result[2].timestamp_us, 300);
    }

    #[test]
    fn scan_isolates_by_property() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let col = Column::new(Arc::clone(&db), TEST_CF);

        let prop_a = Uuid::now_v7();
        let prop_b = Uuid::now_v7();

        col.put(&cell(prop_a, 100, serde_json::json!("a1"))).unwrap();
        col.put(&cell(prop_b, 200, serde_json::json!("b1"))).unwrap();
        col.put(&cell(prop_a, 300, serde_json::json!("a2"))).unwrap();

        let a_cells = col.scan_property(prop_a).unwrap();
        assert_eq!(a_cells.len(), 2);
        assert_eq!(a_cells[0].value, serde_json::json!("a1"));
        assert_eq!(a_cells[1].value, serde_json::json!("a2"));

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
        let c = cell(Uuid::now_v7(), 1_700_000_000_000_000, serde_json::json!(null));
        let key = encode_key(&c);
        assert_eq!(key.len(), 56);

        let (pid, ts, lid, eid) = decode_key(&key).unwrap();
        assert_eq!(pid, c.property_id);
        assert_eq!(ts, c.timestamp_us);
        assert_eq!(lid, c.log_entry_id);
        assert_eq!(eid, c.entity_id);
    }

    #[test]
    fn keys_sort_by_property_then_timestamp() {
        let prop = Uuid::now_v7();
        let lid = Uuid::now_v7();
        let eid = Uuid::now_v7();

        let c1 = ColumnCell { property_id: prop, timestamp_us: 100, log_entry_id: lid, entity_id: eid, value: serde_json::json!(null) };
        let c2 = ColumnCell { property_id: prop, timestamp_us: 200, log_entry_id: lid, entity_id: eid, value: serde_json::json!(null) };

        assert!(encode_key(&c1) < encode_key(&c2));
    }
}
