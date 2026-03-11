use std::sync::Arc;

use rocksdb::DB;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::key::{storage_key, StorageKey};
use super::log::LogError;

/// Status of an investigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InvestigationStatus {
    Open,
    Closed,
}

/// A single versioned record for an investigation.
#[derive(Debug, Clone, Serialize)]
pub struct InvestigationRecord {
    pub id: Uuid,
    pub slug: String,
    pub status: InvestigationStatus,
    pub goal: serde_json::Value,
    pub reasoning: String,
    pub context: serde_json::Value,
    pub notes: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<serde_json::Value>,
    pub tx_id: Uuid,
}

/// Low-level IO for investigation storage. Branch-aware.
pub struct InvestigationIo {
    db: Arc<DB>,
    main_cf: &'static str,
    slug_cf: &'static str,
    branch_id: Uuid,
    ancestry: Vec<(Uuid, Uuid)>,
}

impl InvestigationIo {
    pub(crate) fn new(
        db: Arc<DB>,
        main_cf: &'static str,
        slug_cf: &'static str,
        branch_id: Uuid,
        ancestry: Vec<(Uuid, Uuid)>,
    ) -> Self {
        Self { db, main_cf, slug_cf, branch_id, ancestry }
    }

    /// Writes an investigation record to the main CF.
    pub fn put(&self, record: &InvestigationRecord) -> Result<(), LogError> {
        let main = self.main_cf()?;
        let key = InvestigationKey {
            branch_id: self.branch_id,
            investigation_id: record.id,
            tx_id: record.tx_id,
        }.encode();
        let val_bytes = encode_value(record)?;
        self.db
            .put_cf(main, &key, &val_bytes)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Writes an investigation record + slug index atomically.
    pub fn put_with_index(&self, record: &InvestigationRecord) -> Result<(), LogError> {
        let main = self.main_cf()?;
        let idx = self.slug_cf()?;

        let key = InvestigationKey {
            branch_id: self.branch_id,
            investigation_id: record.id,
            tx_id: record.tx_id,
        }.encode();
        let val_bytes = encode_value(record)?;

        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(main, &key, &val_bytes);
        batch.put_cf(idx, &encode_slug_key(&self.branch_id, &record.slug), record.id.as_bytes());

        self.db
            .write(batch)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Reads the latest record for an investigation by UUID, walking ancestry.
    pub fn get_latest(&self, investigation_id: &Uuid) -> Result<Option<InvestigationRecord>, LogError> {
        let main = self.main_cf()?;
        let mut best: Option<(Uuid, Vec<u8>)> = None;

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let prefix = InvestigationKey::prefix_branch_investigation(&ancestor_id, investigation_id);
            let bound = *branch_point_tx.as_bytes();

            let iter = self.db.prefix_iterator_cf(main, &prefix);
            for item in iter {
                let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
                if !raw_key.starts_with(&prefix) {
                    break;
                }
                let k = InvestigationKey::decode(&raw_key)?;
                if *k.tx_id.as_bytes() > bound {
                    continue;
                }
                match &best {
                    Some((prev_tx, _)) if k.tx_id.as_bytes() <= prev_tx.as_bytes() => {}
                    _ => best = Some((k.tx_id, val.to_vec())),
                }
            }
        }

        match best {
            None => Ok(None),
            Some((tx_id, val_bytes)) => {
                let record = decode_record(investigation_id, tx_id, &val_bytes)?;
                Ok(Some(record))
            }
        }
    }

    /// Looks up a UUID by slug from the index CF, walking ancestry.
    pub fn get_uuid_by_slug(&self, slug: &str) -> Result<Option<Uuid>, LogError> {
        let idx = self.slug_cf()?;
        for &(ancestor_id, _) in &self.ancestry {
            let key = encode_slug_key(&ancestor_id, slug);
            match self.db.get_cf(idx, &key) {
                Ok(Some(bytes)) => {
                    let uuid = Uuid::from_slice(&bytes)
                        .map_err(|e| LogError::Storage(e.to_string()))?;
                    return Ok(Some(uuid));
                }
                Ok(None) => continue,
                Err(e) => return Err(LogError::Storage(e.to_string())),
            }
        }
        Ok(None)
    }

    /// Scans all latest investigation records across ancestry, optionally bounded by tx_id.
    pub fn scan_all_latest_at(&self, upper_bound: Uuid) -> Result<Vec<InvestigationRecord>, LogError> {
        let main = self.main_cf()?;
        let upper_bytes = *upper_bound.as_bytes();
        let mut latest_map: std::collections::HashMap<Uuid, (Uuid, Vec<u8>)> =
            std::collections::HashMap::new();

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let branch_prefix = InvestigationKey::prefix_branch(&ancestor_id);
            let bpt = *branch_point_tx.as_bytes();
            let bound = if upper_bytes < bpt { upper_bytes } else { bpt };

            let iter = self.db.prefix_iterator_cf(main, &branch_prefix);
            for item in iter {
                let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
                if !raw_key.starts_with(&branch_prefix) {
                    break;
                }
                if raw_key.len() < InvestigationKey::SIZE {
                    continue;
                }
                let k = InvestigationKey::decode(&raw_key)?;
                if *k.tx_id.as_bytes() > bound {
                    continue;
                }
                match latest_map.get(&k.investigation_id) {
                    Some((prev_tx, _)) if k.tx_id.as_bytes() <= prev_tx.as_bytes() => {}
                    _ => { latest_map.insert(k.investigation_id, (k.tx_id, val.to_vec())); }
                }
            }
        }

        let mut records = Vec::with_capacity(latest_map.len());
        for (id, (tx_id, val_bytes)) in latest_map {
            records.push(decode_record(&id, tx_id, &val_bytes)?);
        }
        Ok(records)
    }

    fn main_cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.main_cf)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.main_cf)))
    }

    fn slug_cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.slug_cf)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.slug_cf)))
    }
}

storage_key! {
    struct InvestigationKey(48) {
        branch_id: Uuid,
        investigation_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_investigation(branch_id: Uuid, investigation_id: Uuid) -> 32,
    }
}

fn encode_slug_key(branch_id: &Uuid, slug: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(16 + slug.len());
    key.extend_from_slice(branch_id.as_bytes());
    key.extend_from_slice(slug.as_bytes());
    key
}

fn encode_value(record: &InvestigationRecord) -> Result<Vec<u8>, LogError> {
    let val = serde_json::json!({
        "slug": record.slug,
        "status": record.status,
        "goal": record.goal,
        "reasoning": record.reasoning,
        "context": record.context,
        "notes": record.notes,
        "resolution": record.resolution,
    });
    serde_json::to_vec(&val).map_err(|e| LogError::Storage(e.to_string()))
}

fn decode_record(investigation_id: &Uuid, tx_id: Uuid, val_bytes: &[u8]) -> Result<InvestigationRecord, LogError> {
    let val: serde_json::Value = serde_json::from_slice(val_bytes)
        .map_err(|e| LogError::Storage(e.to_string()))?;

    let slug = val.get("slug")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LogError::Storage("missing slug".into()))?
        .to_string();

    let status: InvestigationStatus = val.get("status")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| LogError::Storage(e.to_string()))?
        .unwrap_or(InvestigationStatus::Open);

    let goal = val.get("goal").cloned().unwrap_or(serde_json::Value::Null);
    let reasoning = val.get("reasoning")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let context = val.get("context").cloned().unwrap_or(serde_json::Value::Null);
    let notes = val.get("notes").cloned().unwrap_or(serde_json::Value::Null);
    let resolution = val.get("resolution")
        .and_then(|v| if v.is_null() { None } else { Some(v.clone()) });

    Ok(InvestigationRecord {
        id: *investigation_id,
        slug,
        status,
        goal,
        reasoning,
        context,
        notes,
        resolution,
        tx_id,
    })
}
