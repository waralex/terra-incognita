use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use rocksdb::DB;
use uuid::Uuid;

use crate::schema::{SchemaRegistry, ValueType};

use super::column::{Column, ColumnKey};
use super::key::StorageKey;
use super::log::{AppendLog, LogEntry, LogError, LogKey};
use super::property_value::PropertyValue;
use super::transaction::{Transaction, TransactionStore};

/// Input for a single assertion (or hypothesis) about an entity.
pub struct AssertionInput {
    /// The entity being described.
    pub entity_id: Uuid,
    /// The entity type (used for property validation).
    pub entity_type_id: Uuid,
    /// Property values: property_id → typed value.
    pub properties: HashMap<Uuid, PropertyValue>,
    /// Why this assertion was made — free-form JSON.
    pub reasoning: serde_json::Value,
}

/// Errors from assertion writer operations.
#[derive(Debug, thiserror::Error)]
pub enum WriterError {
    /// Property not attached to entity type.
    #[error("property {property_id} not attached to entity type {entity_type_id}")]
    PropertyNotAttached {
        property_id: Uuid,
        entity_type_id: Uuid,
    },

    /// Property value type does not match schema (e.g. Set value for a Range property).
    #[error("type mismatch for property {property_id}: expected {expected}, got {actual}")]
    TypeMismatch {
        property_id: Uuid,
        expected: &'static str,
        actual: &'static str,
    },

    /// Schema lookup failed.
    #[error("schema error: {0}")]
    Schema(#[from] crate::schema::SchemaError),

    /// Storage-level error.
    #[error(transparent)]
    Storage(#[from] LogError),
}

/// Mid-level writer that validates against schema, writes to the append log,
/// then fans out property values into typed columns — all in one WriteBatch.
pub struct AssertionWriter {
    db: Arc<DB>,
    log: AppendLog,
    tx_store: TransactionStore,
    col_set: Column,
    col_struct: Column,
    col_range: Column,
}

impl AssertionWriter {
    pub(crate) fn new(
        db: Arc<DB>,
        log_cf: &'static str,
        tx_cf: &'static str,
        set_cf: &'static str,
        struct_cf: &'static str,
        range_cf: &'static str,
    ) -> Self {
        Self {
            db: Arc::clone(&db),
            log: AppendLog::new(Arc::clone(&db), log_cf),
            tx_store: TransactionStore::new(Arc::clone(&db), tx_cf),
            col_set: Column::new(Arc::clone(&db), set_cf),
            col_struct: Column::new(Arc::clone(&db), struct_cf),
            col_range: Column::new(db, range_cf),
        }
    }

    /// Writes one or more assertions atomically.
    ///
    /// For each input:
    /// 1. Validates all property_ids belong to the entity type and value types match.
    /// 2. Appends a log entry (body = full property map as JSON).
    /// 3. Fans out each property value into the typed column (set/struct/range).
    ///
    /// Everything goes into a single RocksDB WriteBatch.
    pub fn write(
        &self,
        inputs: &[AssertionInput],
        registry: &SchemaRegistry,
    ) -> Result<Vec<LogEntry>, WriterError> {
        let resolved = self.validate(inputs, registry)?;

        let log_cf = self.log.cf().map_err(WriterError::Storage)?;
        let set_cf = self.col_set.cf().map_err(WriterError::Storage)?;
        let struct_cf = self.col_struct.cf().map_err(WriterError::Storage)?;
        let range_cf = self.col_range.cf().map_err(WriterError::Storage)?;

        let mut batch = rocksdb::WriteBatch::default();
        let mut log_entries = Vec::with_capacity(inputs.len());

        for (input, prop_types) in inputs.iter().zip(resolved.iter()) {
            let now = Utc::now();
            let timestamp_us = now.timestamp_micros();
            let log_entry_id = Uuid::now_v7();

            // Build properties JSON: property_id → typed value
            let mut props_map = serde_json::Map::new();
            for (property_id, value) in &input.properties {
                props_map.insert(property_id.to_string(), value.to_json()?);
            }
            let properties = serde_json::Value::Object(props_map);

            let log_key = LogKey {
                branch_id: super::MAIN_BRANCH,
                timestamp_us,
                entry_id: log_entry_id,
                entity_id: input.entity_id,
            };
            let stored = serde_json::json!({
                "properties": properties,
                "reasoning": input.reasoning,
            });
            let log_val = serde_json::to_vec(&stored)
                .map_err(|e| WriterError::Storage(LogError::Storage(e.to_string())))?;
            batch.put_cf(&log_cf, &log_key.encode(), &log_val);

            // Fan out to typed columns
            for (property_id, value) in &input.properties {
                let vt = prop_types[property_id];
                let col_key = ColumnKey {
                    branch_id: super::MAIN_BRANCH,
                    property_id: *property_id,
                    timestamp_us,
                    log_entry_id,
                    entity_id: input.entity_id,
                };
                let col_val = value.to_bytes()?;

                let cf = match vt {
                    ValueType::Set => &set_cf,
                    ValueType::Struct => &struct_cf,
                    ValueType::Range => &range_cf,
                };
                batch.put_cf(cf, &col_key.encode(), &col_val);
            }

            log_entries.push(LogEntry {
                id: log_entry_id,
                timestamp: now,
                entity_id: input.entity_id,
                tx_id: None,
                properties,
                reasoning: input.reasoning.clone(),
            });
        }

        self.db
            .write(batch)
            .map_err(|e| WriterError::Storage(LogError::Storage(e.to_string())))?;

        Ok(log_entries)
    }

    /// Writes assertions within a transaction, atomically.
    ///
    /// Creates a transaction record with its own reasoning (why this batch was made),
    /// then writes each assertion with its per-value reasoning. Everything goes into
    /// a single WriteBatch: transaction record + log entries + typed columns.
    pub fn write_tx(
        &self,
        entity_id: Uuid,
        tx_reasoning: serde_json::Value,
        inputs: &[AssertionInput],
        registry: &SchemaRegistry,
    ) -> Result<(Transaction, Vec<LogEntry>), WriterError> {
        let resolved = self.validate(inputs, registry)?;

        let log_cf = self.log.cf().map_err(WriterError::Storage)?;
        let set_cf = self.col_set.cf().map_err(WriterError::Storage)?;
        let struct_cf = self.col_struct.cf().map_err(WriterError::Storage)?;
        let range_cf = self.col_range.cf().map_err(WriterError::Storage)?;

        let mut batch = rocksdb::WriteBatch::default();

        let now = Utc::now();
        let tx = Transaction {
            id: Uuid::now_v7(),
            entity_id: Some(entity_id),
            reasoning: tx_reasoning,
            timestamp: now,
        };
        self.tx_store.put_to_batch(&mut batch, &tx)?;

        let mut log_entries = Vec::with_capacity(inputs.len());

        for (input, prop_types) in inputs.iter().zip(resolved.iter()) {
            let timestamp_us = Utc::now().timestamp_micros();
            let log_entry_id = Uuid::now_v7();

            let mut props_map = serde_json::Map::new();
            for (property_id, value) in &input.properties {
                props_map.insert(property_id.to_string(), value.to_json()?);
            }
            let properties = serde_json::Value::Object(props_map);

            let log_key = LogKey {
                branch_id: super::MAIN_BRANCH,
                timestamp_us,
                entry_id: log_entry_id,
                entity_id: input.entity_id,
            };
            let stored = serde_json::json!({
                "properties": properties,
                "reasoning": input.reasoning,
                "tx_id": tx.id.to_string(),
            });
            let log_val = serde_json::to_vec(&stored)
                .map_err(|e| WriterError::Storage(LogError::Storage(e.to_string())))?;
            batch.put_cf(&log_cf, &log_key.encode(), &log_val);

            for (property_id, value) in &input.properties {
                let vt = prop_types[property_id];
                let col_key = ColumnKey {
                    branch_id: super::MAIN_BRANCH,
                    property_id: *property_id,
                    timestamp_us,
                    log_entry_id,
                    entity_id: input.entity_id,
                };
                let col_val = value.to_bytes()?;
                let cf = match vt {
                    ValueType::Set => &set_cf,
                    ValueType::Struct => &struct_cf,
                    ValueType::Range => &range_cf,
                };
                batch.put_cf(cf, &col_key.encode(), &col_val);
            }

            log_entries.push(LogEntry {
                id: log_entry_id,
                timestamp: Utc::now(),
                entity_id: input.entity_id,
                tx_id: Some(tx.id),
                properties,
                reasoning: input.reasoning.clone(),
            });
        }

        self.db
            .write(batch)
            .map_err(|e| WriterError::Storage(LogError::Storage(e.to_string())))?;

        Ok((tx, log_entries))
    }

    /// Writes assertions into an existing WriteBatch with a given tx_id.
    ///
    /// Does NOT create a Transaction record or call `db.write()` — the caller is
    /// responsible for both. Used by multi-entity transactions where one WriteBatch
    /// spans multiple entities and both fact/hypothesis writers.
    pub fn write_to_batch(
        &self,
        batch: &mut rocksdb::WriteBatch,
        tx_id: Uuid,
        inputs: &[AssertionInput],
        registry: &SchemaRegistry,
    ) -> Result<Vec<LogEntry>, WriterError> {
        let resolved = self.validate(inputs, registry)?;

        let log_cf = self.log.cf().map_err(WriterError::Storage)?;
        let set_cf = self.col_set.cf().map_err(WriterError::Storage)?;
        let struct_cf = self.col_struct.cf().map_err(WriterError::Storage)?;
        let range_cf = self.col_range.cf().map_err(WriterError::Storage)?;

        let mut log_entries = Vec::with_capacity(inputs.len());

        for (input, prop_types) in inputs.iter().zip(resolved.iter()) {
            let now = Utc::now();
            let timestamp_us = now.timestamp_micros();
            let log_entry_id = Uuid::now_v7();

            let mut props_map = serde_json::Map::new();
            for (property_id, value) in &input.properties {
                props_map.insert(property_id.to_string(), value.to_json()?);
            }
            let properties = serde_json::Value::Object(props_map);

            let log_key = LogKey {
                branch_id: super::MAIN_BRANCH,
                timestamp_us,
                entry_id: log_entry_id,
                entity_id: input.entity_id,
            };
            let stored = serde_json::json!({
                "properties": properties,
                "reasoning": input.reasoning,
                "tx_id": tx_id.to_string(),
            });
            let log_val = serde_json::to_vec(&stored)
                .map_err(|e| WriterError::Storage(LogError::Storage(e.to_string())))?;
            batch.put_cf(&log_cf, &log_key.encode(), &log_val);

            for (property_id, value) in &input.properties {
                let vt = prop_types[property_id];
                let col_key = ColumnKey {
                    branch_id: super::MAIN_BRANCH,
                    property_id: *property_id,
                    timestamp_us,
                    log_entry_id,
                    entity_id: input.entity_id,
                };
                let col_val = value.to_bytes()?;
                let cf = match vt {
                    ValueType::Set => &set_cf,
                    ValueType::Struct => &struct_cf,
                    ValueType::Range => &range_cf,
                };
                batch.put_cf(cf, &col_key.encode(), &col_val);
            }

            log_entries.push(LogEntry {
                id: log_entry_id,
                timestamp: now,
                entity_id: input.entity_id,
                tx_id: Some(tx_id),
                properties,
                reasoning: input.reasoning.clone(),
            });
        }

        Ok(log_entries)
    }

    /// Validates all inputs: properties belong to entity type and value types match.
    fn validate(
        &self,
        inputs: &[AssertionInput],
        registry: &SchemaRegistry,
    ) -> Result<Vec<HashMap<Uuid, ValueType>>, WriterError> {
        let mut result = Vec::with_capacity(inputs.len());

        for input in inputs {
            let schema_props = registry.list_properties_by_type_id(&input.entity_type_id)?;
            let allowed: HashMap<Uuid, ValueType> = schema_props
                .into_iter()
                .map(|p| (p.id, p.value_type))
                .collect();

            for (property_id, value) in &input.properties {
                let expected_vt = allowed.get(property_id).ok_or(
                    WriterError::PropertyNotAttached {
                        property_id: *property_id,
                        entity_type_id: input.entity_type_id,
                    },
                )?;

                let actual_vt = value_type_name(value);
                let expected_name = match expected_vt {
                    ValueType::Set => "set",
                    ValueType::Struct => "struct",
                    ValueType::Range => "range",
                };
                if actual_vt != expected_name {
                    return Err(WriterError::TypeMismatch {
                        property_id: *property_id,
                        expected: expected_name,
                        actual: actual_vt,
                    });
                }
            }

            result.push(allowed);
        }

        Ok(result)
    }
}

fn value_type_name(value: &PropertyValue) -> &'static str {
    match value {
        PropertyValue::Set(_) => "set",
        PropertyValue::Struct(_) => "struct",
        PropertyValue::Range(_) => "range",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::property_value::{RangeValue, SetValue, StructValue};
    use crate::assertion::AssertionStore;
    use crate::schema::SchemaRegistry;
    use serde_json::json;

    fn setup() -> (AssertionStore, SchemaRegistry, tempfile::TempDir) {
        let reg = SchemaRegistry::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (store, reg, dir)
    }

    fn create_schema(reg: &SchemaRegistry) -> (Uuid, Uuid, Uuid, Uuid) {
        let et = reg.create_entity_type("track", None).unwrap();
        let p_bpm = reg
            .create_property("bpm", ValueType::Range, None)
            .unwrap();
        let p_cert = reg
            .create_property("certification", ValueType::Set, None)
            .unwrap();
        let p_meta = reg
            .create_property("metadata", ValueType::Struct, None)
            .unwrap();
        reg.attach_property("track", "bpm").unwrap();
        reg.attach_property("track", "certification").unwrap();
        reg.attach_property("track", "metadata").unwrap();
        (et.id, p_bpm.id, p_cert.id, p_meta.id)
    }

    fn range_eq(v: serde_json::Value) -> PropertyValue {
        PropertyValue::Range(RangeValue::Eq(v))
    }

    fn set_contains(v: Vec<serde_json::Value>) -> PropertyValue {
        PropertyValue::Set(SetValue {
            contains: v,
            not_contains: vec![],
        })
    }

    fn struct_val(v: serde_json::Value) -> PropertyValue {
        PropertyValue::Struct(StructValue(v))
    }

    #[test]
    fn write_single_assertion() {
        let (store, reg, _dir) = setup();
        let (et_id, p_bpm, p_cert, _p_meta) = create_schema(&reg);
        let writer = store.fact_writer();

        let entity_id = Uuid::now_v7();
        let props = HashMap::from([
            (p_bpm, range_eq(json!(120))),
            (p_cert, set_contains(vec![json!("gold")])),
        ]);

        let entries = writer
            .write(
                &[AssertionInput {
                    entity_id,
                    entity_type_id: et_id,
                    properties: props,
                    reasoning: serde_json::json!(null),
                }],
                &reg,
            )
            .unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entity_id, entity_id);

        let log = store.facts().list().unwrap();
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn write_fans_out_to_columns() {
        let (store, reg, _dir) = setup();
        let (et_id, p_bpm, p_cert, p_meta) = create_schema(&reg);
        let writer = store.fact_writer();

        let entity_id = Uuid::now_v7();
        let props = HashMap::from([
            (p_bpm, range_eq(json!(140))),
            (p_cert, set_contains(vec![json!("platinum")])),
            (p_meta, struct_val(json!({"genre": "pop"}))),
        ]);

        writer
            .write(
                &[AssertionInput {
                    entity_id,
                    entity_type_id: et_id,
                    properties: props,
                    reasoning: serde_json::json!(null),
                }],
                &reg,
            )
            .unwrap();

        let range_col = store.fact_col_range();
        let range_cells = range_col.scan_property(p_bpm).unwrap();
        assert_eq!(range_cells.len(), 1);
        assert_eq!(range_cells[0].value, json!({"eq": 140}));

        let set_col = store.fact_col_set();
        let set_cells = set_col.scan_property(p_cert).unwrap();
        assert_eq!(set_cells.len(), 1);
        assert_eq!(set_cells[0].value, json!({"contains": ["platinum"]}));

        let struct_col = store.fact_col_struct();
        let struct_cells = struct_col.scan_property(p_meta).unwrap();
        assert_eq!(struct_cells.len(), 1);
        assert_eq!(struct_cells[0].value, json!({"genre": "pop"}));
    }

    #[test]
    fn rejects_unknown_property() {
        let (store, reg, _dir) = setup();
        let (et_id, _p_bpm, _p_cert, _p_meta) = create_schema(&reg);
        let writer = store.fact_writer();

        let bogus_prop = Uuid::now_v7();
        let props = HashMap::from([(bogus_prop, range_eq(json!(0)))]);

        let err = writer
            .write(
                &[AssertionInput {
                    entity_id: Uuid::now_v7(),
                    entity_type_id: et_id,
                    properties: props,
                    reasoning: serde_json::json!(null),
                }],
                &reg,
            )
            .unwrap_err();

        assert!(matches!(err, WriterError::PropertyNotAttached { .. }));

        let log = store.facts().list().unwrap();
        assert!(log.is_empty());
    }

    #[test]
    fn rejects_type_mismatch() {
        let (store, reg, _dir) = setup();
        let (et_id, p_bpm, _p_cert, _p_meta) = create_schema(&reg);
        let writer = store.fact_writer();

        // p_bpm is Range, but we pass a Set value
        let props = HashMap::from([(p_bpm, set_contains(vec![json!("wrong")]))]);

        let err = writer
            .write(
                &[AssertionInput {
                    entity_id: Uuid::now_v7(),
                    entity_type_id: et_id,
                    properties: props,
                    reasoning: serde_json::json!(null),
                }],
                &reg,
            )
            .unwrap_err();

        assert!(matches!(err, WriterError::TypeMismatch { .. }));
    }

    #[test]
    fn batch_write_is_atomic() {
        let (store, reg, _dir) = setup();
        let (et_id, p_bpm, _p_cert, _p_meta) = create_schema(&reg);
        let writer = store.fact_writer();

        let entries = writer
            .write(
                &[
                    AssertionInput {
                        entity_id: Uuid::now_v7(),
                        entity_type_id: et_id,
                        properties: HashMap::from([(p_bpm, range_eq(json!(100)))]),
                        reasoning: json!(null),
                    },
                    AssertionInput {
                        entity_id: Uuid::now_v7(),
                        entity_type_id: et_id,
                        properties: HashMap::from([(p_bpm, range_eq(json!(200)))]),
                        reasoning: json!(null),
                    },
                ],
                &reg,
            )
            .unwrap();

        assert_eq!(entries.len(), 2);

        let log = store.facts().list().unwrap();
        assert_eq!(log.len(), 2);

        let range_col = store.fact_col_range();
        let cells = range_col.scan_property(p_bpm).unwrap();
        assert_eq!(cells.len(), 2);
    }

    #[test]
    fn hypothesis_writer_uses_separate_log() {
        let (store, reg, _dir) = setup();
        let (et_id, p_bpm, _p_cert, _p_meta) = create_schema(&reg);

        let fact_writer = store.fact_writer();
        let hyp_writer = store.hypothesis_writer();

        fact_writer
            .write(
                &[AssertionInput {
                    entity_id: Uuid::now_v7(),
                    entity_type_id: et_id,
                    properties: HashMap::from([(p_bpm, range_eq(json!(120)))]),
                    reasoning: json!(null),
                }],
                &reg,
            )
            .unwrap();

        hyp_writer
            .write(
                &[AssertionInput {
                    entity_id: Uuid::now_v7(),
                    entity_type_id: et_id,
                    properties: HashMap::from([(p_bpm, range_eq(json!(130)))]),
                    reasoning: json!(null),
                }],
                &reg,
            )
            .unwrap();

        assert_eq!(store.facts().list().unwrap().len(), 1);
        assert_eq!(store.hypotheses().list().unwrap().len(), 1);
    }

    #[test]
    fn write_tx_creates_transaction_and_entries() {
        let (store, reg, _dir) = setup();
        let (et_id, p_bpm, p_cert, _p_meta) = create_schema(&reg);
        let writer = store.fact_writer();

        let entity_id = Uuid::now_v7();
        let (tx, entries) = writer
            .write_tx(
                entity_id,
                json!("analyzed spectral data and narrowed hypotheses"),
                &[
                    AssertionInput {
                        entity_id,
                        entity_type_id: et_id,
                        properties: HashMap::from([(p_bpm, range_eq(json!(128)))]),
                        reasoning: json!("BPM detected from audio analysis"),
                    },
                    AssertionInput {
                        entity_id,
                        entity_type_id: et_id,
                        properties: HashMap::from([(p_cert, set_contains(vec![json!("gold")]))]),
                        reasoning: json!("certification confirmed by RIAA database"),
                    },
                ],
                &reg,
            )
            .unwrap();

        assert_eq!(tx.entity_id, Some(entity_id));
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.tx_id == Some(tx.id)));

        // Transaction is retrievable
        let loaded_tx = store.transactions().get(&tx.id).unwrap().unwrap();
        assert_eq!(loaded_tx.entity_id, Some(entity_id));
        assert_eq!(loaded_tx.reasoning, json!("analyzed spectral data and narrowed hypotheses"));

        // Log entries have tx_id
        let log = store.facts().list().unwrap();
        assert_eq!(log.len(), 2);
        assert!(log.iter().all(|e| e.tx_id == Some(tx.id)));
    }

    #[test]
    fn write_tx_fans_out_to_columns() {
        let (store, reg, _dir) = setup();
        let (et_id, p_bpm, _p_cert, _p_meta) = create_schema(&reg);
        let writer = store.fact_writer();

        let entity_id = Uuid::now_v7();
        writer
            .write_tx(
                entity_id,
                json!(null),
                &[AssertionInput {
                    entity_id,
                    entity_type_id: et_id,
                    properties: HashMap::from([(p_bpm, range_eq(json!(160)))]),
                    reasoning: json!(null),
                }],
                &reg,
            )
            .unwrap();

        let range_col = store.fact_col_range();
        let cells = range_col.scan_property(p_bpm).unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].value, json!({"eq": 160}));
    }

    #[test]
    fn write_without_tx_has_no_tx_id() {
        let (store, reg, _dir) = setup();
        let (et_id, p_bpm, _p_cert, _p_meta) = create_schema(&reg);
        let writer = store.fact_writer();

        writer
            .write(
                &[AssertionInput {
                    entity_id: Uuid::now_v7(),
                    entity_type_id: et_id,
                    properties: HashMap::from([(p_bpm, range_eq(json!(90)))]),
                    reasoning: json!(null),
                }],
                &reg,
            )
            .unwrap();

        let log = store.facts().list().unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].tx_id.is_none());
    }
}
