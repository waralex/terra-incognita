use std::sync::Arc;

use rocksdb::DB;
use uuid::Uuid;

use super::key::{storage_key, StorageKey};
use super::log::LogError;

/// Kind of item subject to visibility control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ItemKind {
    Entity = 0,
    EntityType = 1,
    Property = 2,
}

storage_key! {
    pub(crate) struct VisibilityKey(49) {
        branch_id: Uuid,
        tx_id: Uuid,
        item_kind: u8,
        item_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
    }
}

/// Low-level visibility storage backed by a RocksDB column family.
///
/// Key: `branch_id(16) | tx_id(16) | item_kind(1) | item_id(16)` = 49 bytes.
/// Value: single byte — `1` = hidden, `0` = visible.
///
/// Default (no record) = visible.
pub struct VisibilityStore {
    db: Arc<DB>,
    cf_name: &'static str,
}

impl VisibilityStore {
    pub(crate) fn new(db: Arc<DB>, cf_name: &'static str) -> Self {
        Self { db, cf_name }
    }

    /// Writes hide records for multiple items into a WriteBatch.
    pub fn hide_to_batch(
        &self,
        batch: &mut rocksdb::WriteBatch,
        branch_id: Uuid,
        tx_id: Uuid,
        kind: ItemKind,
        item_ids: &[Uuid],
    ) -> Result<(), LogError> {
        let cf = self.cf()?;
        for &item_id in item_ids {
            let key = VisibilityKey {
                branch_id,
                tx_id,
                item_kind: kind as u8,
                item_id,
            };
            batch.put_cf(cf, key.encode(), &[1u8]);
        }
        Ok(())
    }

    /// Writes unhide records for multiple items into a WriteBatch.
    pub fn unhide_to_batch(
        &self,
        batch: &mut rocksdb::WriteBatch,
        branch_id: Uuid,
        tx_id: Uuid,
        kind: ItemKind,
        item_ids: &[Uuid],
    ) -> Result<(), LogError> {
        let cf = self.cf()?;
        for &item_id in item_ids {
            let key = VisibilityKey {
                branch_id,
                tx_id,
                item_kind: kind as u8,
                item_id,
            };
            batch.put_cf(cf, key.encode(), &[0u8]);
        }
        Ok(())
    }

    /// Checks if an item is visible by walking the ancestry chain.
    ///
    /// Scans visibility records on each ancestor branch (filtered by branch_point_tx).
    /// The latest record wins. Default (no records) = visible.
    pub fn is_visible(
        &self,
        ancestry: &[(Uuid, Uuid)],
        kind: ItemKind,
        item_id: Uuid,
    ) -> Result<bool, LogError> {
        let cf = self.cf()?;
        let kind_byte = kind as u8;

        for &(branch_id, branch_point_tx) in ancestry {
            let prefix = VisibilityKey::prefix_branch(&branch_id);
            let iter = self.db.prefix_iterator_cf(cf, &prefix);

            let mut latest_tx: Option<(Uuid, bool)> = None;

            for item in iter {
                let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
                if !raw_key.starts_with(&prefix) {
                    break;
                }
                let k = VisibilityKey::decode(&raw_key)?;

                // Filter by branch point
                if k.tx_id.as_bytes() > branch_point_tx.as_bytes() {
                    continue;
                }

                // Filter by kind and item
                if k.item_kind != kind_byte || k.item_id != item_id {
                    continue;
                }

                let hidden = val.first().copied().unwrap_or(0) == 1;
                match &latest_tx {
                    Some((prev_tx, _)) if k.tx_id.as_bytes() <= prev_tx.as_bytes() => {}
                    _ => latest_tx = Some((k.tx_id, hidden)),
                }
            }

            if let Some((_, hidden)) = latest_tx {
                return Ok(!hidden);
            }
        }

        // No visibility record found — default visible
        Ok(true)
    }

    fn cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.cf_name)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.cf_name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocksdb::{ColumnFamilyDescriptor, Options, DB};

    const TEST_CF: &str = "test_visibility";

    fn open_db(dir: &tempfile::TempDir) -> Arc<DB> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let mut cf_opts = Options::default();
        cf_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));
        let cf = ColumnFamilyDescriptor::new(TEST_CF, cf_opts);
        Arc::new(DB::open_cf_descriptors(&opts, dir.path(), vec![cf]).unwrap())
    }

    #[test]
    fn default_is_visible() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = VisibilityStore::new(db, TEST_CF);

        let branch = Uuid::now_v7();
        let ancestry = vec![(branch, Uuid::max())];
        let entity_id = Uuid::now_v7();

        assert!(store.is_visible(&ancestry, ItemKind::Entity, entity_id).unwrap());
    }

    #[test]
    fn hide_makes_invisible() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = VisibilityStore::new(Arc::clone(&db), TEST_CF);

        let branch = Uuid::now_v7();
        let tx = Uuid::now_v7();
        let entity_id = Uuid::now_v7();
        let ancestry = vec![(branch, Uuid::max())];

        let mut batch = rocksdb::WriteBatch::default();
        store.hide_to_batch(&mut batch, branch, tx, ItemKind::Entity, &[entity_id]).unwrap();
        db.write(batch).unwrap();

        assert!(!store.is_visible(&ancestry, ItemKind::Entity, entity_id).unwrap());
    }

    #[test]
    fn unhide_after_hide_restores_visibility() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = VisibilityStore::new(Arc::clone(&db), TEST_CF);

        let branch = Uuid::now_v7();
        let entity_id = Uuid::now_v7();
        let ancestry = vec![(branch, Uuid::max())];

        let tx1 = Uuid::now_v7();
        let mut batch = rocksdb::WriteBatch::default();
        store.hide_to_batch(&mut batch, branch, tx1, ItemKind::Entity, &[entity_id]).unwrap();
        db.write(batch).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1));

        let tx2 = Uuid::now_v7();
        let mut batch = rocksdb::WriteBatch::default();
        store.unhide_to_batch(&mut batch, branch, tx2, ItemKind::Entity, &[entity_id]).unwrap();
        db.write(batch).unwrap();

        assert!(store.is_visible(&ancestry, ItemKind::Entity, entity_id).unwrap());
    }

    #[test]
    fn visibility_respects_branch_point() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = VisibilityStore::new(Arc::clone(&db), TEST_CF);

        let parent = Uuid::now_v7();
        let entity_id = Uuid::now_v7();

        // Hide before branch point
        let tx1 = Uuid::now_v7();
        let mut batch = rocksdb::WriteBatch::default();
        store.hide_to_batch(&mut batch, parent, tx1, ItemKind::Entity, &[entity_id]).unwrap();
        db.write(batch).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1));
        let branch_point = Uuid::now_v7();

        std::thread::sleep(std::time::Duration::from_millis(1));

        // Unhide after branch point
        let tx2 = Uuid::now_v7();
        let mut batch = rocksdb::WriteBatch::default();
        store.unhide_to_batch(&mut batch, parent, tx2, ItemKind::Entity, &[entity_id]).unwrap();
        db.write(batch).unwrap();

        // Child branch sees only up to branch_point — entity is hidden
        let child = Uuid::now_v7();
        let ancestry = vec![(child, Uuid::max()), (parent, branch_point)];
        assert!(!store.is_visible(&ancestry, ItemKind::Entity, entity_id).unwrap());

        // Parent sees everything — entity is visible (unhidden)
        let parent_ancestry = vec![(parent, Uuid::max())];
        assert!(store.is_visible(&parent_ancestry, ItemKind::Entity, entity_id).unwrap());
    }

    #[test]
    fn different_kinds_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = VisibilityStore::new(Arc::clone(&db), TEST_CF);

        let branch = Uuid::now_v7();
        let id = Uuid::now_v7();
        let tx = Uuid::now_v7();
        let ancestry = vec![(branch, Uuid::max())];

        let mut batch = rocksdb::WriteBatch::default();
        store.hide_to_batch(&mut batch, branch, tx, ItemKind::Entity, &[id]).unwrap();
        db.write(batch).unwrap();

        // Entity is hidden, but same UUID as EntityType is still visible
        assert!(!store.is_visible(&ancestry, ItemKind::Entity, id).unwrap());
        assert!(store.is_visible(&ancestry, ItemKind::EntityType, id).unwrap());
    }

    #[test]
    fn key_encoding_roundtrip() {
        let key = VisibilityKey {
            branch_id: Uuid::now_v7(),
            tx_id: Uuid::now_v7(),
            item_kind: ItemKind::Property as u8,
            item_id: Uuid::now_v7(),
        };
        let encoded = key.encode();
        assert_eq!(encoded.len(), VisibilityKey::SIZE);

        let decoded = VisibilityKey::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }
}
