use std::sync::Arc;

use chrono::{DateTime, Utc};
use rocksdb::DB;
use uuid::Uuid;

use super::entity_property::{EntityProperty, ValueType};
use super::entity_type::EntityType;
use super::reserved;
use super::slug::validate_slug;
use super::SchemaError;

use crate::assertion::key::{storage_key, StorageKey};

storage_key! {
    pub(crate) struct SchemaTypeKey(48) {
        branch_id: Uuid,
        type_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_type(branch_id: Uuid, type_id: Uuid) -> 32,
    }
}

storage_key! {
    pub(crate) struct SchemaPropertyKey(48) {
        branch_id: Uuid,
        prop_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_prop(branch_id: Uuid, prop_id: Uuid) -> 32,
    }
}

storage_key! {
    pub(crate) struct SchemaAttachmentKey(64) {
        branch_id: Uuid,
        type_id: Uuid,
        tx_id: Uuid,
        prop_id: Uuid,
    }
    prefixes {
        prefix_branch_type(branch_id: Uuid, type_id: Uuid) -> 32,
    }
}

/// Input for creating an entity type (single item in a batch).
pub struct EntityTypeInput<'a> {
    pub slug: &'a str,
    pub description: Option<&'a str>,
    pub properties: &'a [&'a str],
}

/// Input for creating a property (single item in a batch).
pub struct PropertyInput<'a> {
    pub slug: &'a str,
    pub value_type: ValueType,
    pub description: Option<&'a str>,
    pub entity_types: &'a [&'a str],
}

/// Input for attaching an existing property to an existing entity type.
pub struct AttachInput<'a> {
    pub entity_type: &'a str,
    pub property: &'a str,
}

/// Serialized entity type value stored in RocksDB.
#[derive(serde::Serialize, serde::Deserialize)]
struct EntityTypeValue {
    slug: String,
    description: Option<String>,
    created_at: String,
}

/// Serialized property value stored in RocksDB.
#[derive(serde::Serialize, serde::Deserialize)]
struct PropertyValue {
    slug: String,
    description: Option<String>,
    value_type: String,
    created_at: String,
}

/// RocksDB-backed schema registry scoped to a branch with ancestry chain walk.
pub struct BranchSchemaRegistry {
    db: Arc<DB>,
    branch_id: Uuid,
    /// `(branch_id, branch_point_tx)` — for temporal filtering on reads via UUID byte comparison.
    ancestry: Vec<(Uuid, Uuid)>,
}

impl BranchSchemaRegistry {
    /// Creates a registry for the given branch with its resolved ancestry.
    pub fn new(db: Arc<DB>, branch_id: Uuid, ancestry: Vec<(Uuid, Uuid)>) -> Self {
        Self { db, branch_id, ancestry }
    }

    // -- Entity Types --

    /// Creates a single entity type in the current branch.
    pub fn create_entity_type(
        &self,
        slug: &str,
        description: Option<&str>,
    ) -> Result<EntityType, SchemaError> {
        validate_slug(slug)?;

        // Check slug uniqueness across ancestry
        if self.get_entity_type(slug).is_ok() {
            return Err(SchemaError::DuplicateEntityType(slug.to_string()));
        }

        let id = Uuid::now_v7();
        let now = Utc::now();

        self.write_entity_type(id, slug, description, &now)?;
        self.write_type_slug_index(slug, &id)?;

        Ok(EntityType {
            id,
            slug: slug.to_string(),
            description: description.map(String::from),
            created_at: now,
        })
    }

    /// Creates entity types in batch. All-or-nothing via WriteBatch.
    pub fn create_entity_types_batch(
        &self,
        items: &[EntityTypeInput<'_>],
    ) -> Result<Vec<EntityType>, SchemaError> {
        // Validate all first
        for (i, item) in items.iter().enumerate() {
            validate_slug(item.slug).map_err(|e| SchemaError::BatchItemError {
                index: i,
                source: Box::new(e),
            })?;
            if self.get_entity_type(item.slug).is_ok() {
                return Err(SchemaError::BatchItemError {
                    index: i,
                    source: Box::new(SchemaError::DuplicateEntityType(item.slug.to_string())),
                });
            }
            // Check for duplicate slugs within the batch
            for prev in &items[..i] {
                if prev.slug == item.slug {
                    return Err(SchemaError::BatchItemError {
                        index: i,
                        source: Box::new(SchemaError::DuplicateEntityType(item.slug.to_string())),
                    });
                }
            }
            for prop_slug in item.properties {
                if self.get_property_by_slug(prop_slug).is_err() {
                    return Err(SchemaError::BatchItemError {
                        index: i,
                        source: Box::new(SchemaError::PropertyNotFound(prop_slug.to_string())),
                    });
                }
            }
        }

        let mut batch = rocksdb::WriteBatch::default();
        let mut results = Vec::with_capacity(items.len());

        let type_cf = self.cf("schema_types")?;
        let type_slug_cf = self.cf("schema_type_slug")?;
        let attach_cf = self.cf("schema_attachments")?;

        for item in items {
            let id = Uuid::now_v7();
            let now = Utc::now();
            let tx_id = Uuid::now_v7();

            let val = EntityTypeValue {
                slug: item.slug.to_string(),
                description: item.description.map(String::from),
                created_at: now.to_rfc3339(),
            };
            let key = SchemaTypeKey { branch_id: self.branch_id, type_id: id, tx_id }.encode();
            let val_bytes = serde_json::to_vec(&val)
                .map_err(|e| SchemaError::Storage(e.to_string()))?;
            batch.put_cf(type_cf, &key, &val_bytes);

            let slug_key = encode_branch_slug_key(&self.branch_id, item.slug);
            batch.put_cf(type_slug_cf, &slug_key, id.as_bytes());

            for prop_slug in item.properties {
                let prop = self.get_property_by_slug(prop_slug)?;
                let attach_tx_id = Uuid::now_v7();
                let attach_key = SchemaAttachmentKey {
                    branch_id: self.branch_id,
                    type_id: id,
                    tx_id: attach_tx_id,
                    prop_id: prop.id,
                }.encode();
                batch.put_cf(attach_cf, &attach_key, &[]);
            }

            results.push(EntityType {
                id,
                slug: item.slug.to_string(),
                description: item.description.map(String::from),
                created_at: now,
            });
        }

        self.db.write(batch).map_err(|e| SchemaError::Storage(e.to_string()))?;
        Ok(results)
    }

    /// Lists all entity types visible from this branch (walking ancestry).
    pub fn list_entity_types(&self) -> Result<Vec<EntityType>, SchemaError> {
        let cf = self.cf("schema_types")?;
        let mut types = Vec::new();
        let mut seen_slugs = std::collections::HashSet::new();

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let prefix = SchemaTypeKey::prefix_branch(&ancestor_id);
            let iter = self.db.prefix_iterator_cf(cf, &prefix);
            for item in iter {
                let (raw_key, val) = item.map_err(|e| SchemaError::Storage(e.to_string()))?;
                if !raw_key.starts_with(&prefix) { break; }

                let k = SchemaTypeKey::decode(&raw_key)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;
                if k.tx_id.as_bytes() > branch_point_tx.as_bytes() { continue; }

                let et_val: EntityTypeValue = serde_json::from_slice(&val)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;

                if seen_slugs.insert(et_val.slug.clone()) {
                    let created_at = parse_datetime(&et_val.created_at)?;
                    types.push(EntityType {
                        id: k.type_id,
                        slug: et_val.slug,
                        description: et_val.description,
                        created_at,
                    });
                }
            }
        }

        types.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(types)
    }

    /// Gets an entity type by slug (chain walk).
    pub fn get_entity_type(&self, slug: &str) -> Result<EntityType, SchemaError> {
        let slug_cf = self.cf("schema_type_slug")?;
        let type_cf = self.cf("schema_types")?;

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let slug_key = encode_branch_slug_key(&ancestor_id, slug);
            if let Some(id_bytes) = self.db.get_cf(slug_cf, &slug_key)
                .map_err(|e| SchemaError::Storage(e.to_string()))? {
                let type_id = Uuid::from_slice(&id_bytes)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;
                // Prefix scan on (branch_id, type_id) — schema items are immutable,
                // so at most one entry per (branch_id, type_id).
                let prefix = SchemaTypeKey::prefix_branch_type(&ancestor_id, &type_id);
                let iter = self.db.prefix_iterator_cf(type_cf, &prefix);
                for item in iter {
                    let (raw_key, val_bytes) = item.map_err(|e| SchemaError::Storage(e.to_string()))?;
                    if !raw_key.starts_with(&prefix) { break; }
                    let k = SchemaTypeKey::decode(&raw_key)
                        .map_err(|e| SchemaError::Storage(e.to_string()))?;
                    if k.tx_id.as_bytes() <= branch_point_tx.as_bytes() {
                        let et_val: EntityTypeValue = serde_json::from_slice(&val_bytes)
                            .map_err(|e| SchemaError::Storage(e.to_string()))?;
                        let created_at = parse_datetime(&et_val.created_at)?;
                        return Ok(EntityType {
                            id: type_id,
                            slug: et_val.slug,
                            description: et_val.description,
                            created_at,
                        });
                    }
                }
            }
        }

        Err(SchemaError::EntityTypeNotFound(slug.to_string()))
    }

    /// Gets an entity type by UUID (chain walk).
    pub fn get_entity_type_by_id(&self, id: &Uuid) -> Result<EntityType, SchemaError> {
        let type_cf = self.cf("schema_types")?;

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let prefix = SchemaTypeKey::prefix_branch_type(&ancestor_id, id);
            let iter = self.db.prefix_iterator_cf(type_cf, &prefix);
            for item in iter {
                let (raw_key, val_bytes) = item.map_err(|e| SchemaError::Storage(e.to_string()))?;
                if !raw_key.starts_with(&prefix) { break; }
                let k = SchemaTypeKey::decode(&raw_key)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;
                if k.tx_id.as_bytes() <= branch_point_tx.as_bytes() {
                    let et_val: EntityTypeValue = serde_json::from_slice(&val_bytes)
                        .map_err(|e| SchemaError::Storage(e.to_string()))?;
                    let created_at = parse_datetime(&et_val.created_at)?;
                    return Ok(EntityType {
                        id: *id,
                        slug: et_val.slug,
                        description: et_val.description,
                        created_at,
                    });
                }
            }
        }

        Err(SchemaError::EntityTypeNotFound(id.to_string()))
    }

    // -- Properties --

    /// Creates a single property in the current branch.
    pub fn create_property(
        &self,
        slug: &str,
        value_type: ValueType,
        description: Option<&str>,
    ) -> Result<EntityProperty, SchemaError> {
        validate_slug(slug)?;
        if reserved::is_reserved(slug) {
            return Err(SchemaError::ReservedProperty(slug.to_string()));
        }

        if self.get_property_by_slug(slug).is_ok() {
            return Err(SchemaError::DuplicateProperty(slug.to_string()));
        }

        let id = Uuid::now_v7();
        let now = Utc::now();

        self.write_property(id, slug, value_type, description, &now)?;
        self.write_prop_slug_index(slug, &id)?;

        Ok(EntityProperty {
            id,
            slug: slug.to_string(),
            description: description.map(String::from),
            value_type,
            created_at: now,
        })
    }

    /// Creates properties in batch. All-or-nothing via WriteBatch.
    pub fn create_properties_batch(
        &self,
        items: &[PropertyInput<'_>],
    ) -> Result<Vec<EntityProperty>, SchemaError> {
        for (i, item) in items.iter().enumerate() {
            validate_slug(item.slug).map_err(|e| SchemaError::BatchItemError {
                index: i,
                source: Box::new(e),
            })?;
            if reserved::is_reserved(item.slug) {
                return Err(SchemaError::BatchItemError {
                    index: i,
                    source: Box::new(SchemaError::ReservedProperty(item.slug.to_string())),
                });
            }
            if self.get_property_by_slug(item.slug).is_ok() {
                return Err(SchemaError::BatchItemError {
                    index: i,
                    source: Box::new(SchemaError::DuplicateProperty(item.slug.to_string())),
                });
            }
            for prev in &items[..i] {
                if prev.slug == item.slug {
                    return Err(SchemaError::BatchItemError {
                        index: i,
                        source: Box::new(SchemaError::DuplicateProperty(item.slug.to_string())),
                    });
                }
            }
            for et_slug in item.entity_types {
                if self.get_entity_type(et_slug).is_err() {
                    return Err(SchemaError::BatchItemError {
                        index: i,
                        source: Box::new(SchemaError::EntityTypeNotFound(et_slug.to_string())),
                    });
                }
            }
        }

        let mut batch = rocksdb::WriteBatch::default();
        let mut results = Vec::with_capacity(items.len());

        let prop_cf = self.cf("schema_props")?;
        let prop_slug_cf = self.cf("schema_prop_slug")?;
        let attach_cf = self.cf("schema_attachments")?;

        for item in items {
            let id = Uuid::now_v7();
            let now = Utc::now();
            let tx_id = Uuid::now_v7();

            let val = PropertyValue {
                slug: item.slug.to_string(),
                description: item.description.map(String::from),
                value_type: item.value_type.as_str().to_string(),
                created_at: now.to_rfc3339(),
            };
            let key = SchemaPropertyKey { branch_id: self.branch_id, prop_id: id, tx_id }.encode();
            let val_bytes = serde_json::to_vec(&val)
                .map_err(|e| SchemaError::Storage(e.to_string()))?;
            batch.put_cf(prop_cf, &key, &val_bytes);

            let slug_key = encode_branch_slug_key(&self.branch_id, item.slug);
            batch.put_cf(prop_slug_cf, &slug_key, id.as_bytes());

            for et_slug in item.entity_types {
                let et = self.get_entity_type(et_slug)?;
                let attach_tx_id = Uuid::now_v7();
                let attach_key = SchemaAttachmentKey {
                    branch_id: self.branch_id,
                    type_id: et.id,
                    tx_id: attach_tx_id,
                    prop_id: id,
                }.encode();
                batch.put_cf(attach_cf, &attach_key, &[]);
            }

            results.push(EntityProperty {
                id,
                slug: item.slug.to_string(),
                description: item.description.map(String::from),
                value_type: item.value_type,
                created_at: now,
            });
        }

        self.db.write(batch).map_err(|e| SchemaError::Storage(e.to_string()))?;
        Ok(results)
    }

    /// Lists all properties visible from this branch.
    pub fn list_all_properties(&self) -> Result<Vec<EntityProperty>, SchemaError> {
        let cf = self.cf("schema_props")?;
        let mut props = Vec::new();
        let mut seen_slugs = std::collections::HashSet::new();

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let prefix = SchemaPropertyKey::prefix_branch(&ancestor_id);
            let iter = self.db.prefix_iterator_cf(cf, &prefix);
            for item in iter {
                let (raw_key, val) = item.map_err(|e| SchemaError::Storage(e.to_string()))?;
                if !raw_key.starts_with(&prefix) { break; }

                let k = SchemaPropertyKey::decode(&raw_key)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;
                if k.tx_id.as_bytes() > branch_point_tx.as_bytes() { continue; }

                let pv: PropertyValue = serde_json::from_slice(&val)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;

                if seen_slugs.insert(pv.slug.clone()) {
                    let created_at = parse_datetime(&pv.created_at)?;
                    let value_type = ValueType::from_str(&pv.value_type)
                        .ok_or_else(|| SchemaError::Storage(format!("invalid value_type: {}", pv.value_type)))?;
                    props.push(EntityProperty {
                        id: k.prop_id,
                        slug: pv.slug,
                        description: pv.description,
                        value_type,
                        created_at,
                    });
                }
            }
        }

        props.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(props)
    }

    /// Gets a property by slug (chain walk).
    pub fn get_property_by_slug(&self, slug: &str) -> Result<EntityProperty, SchemaError> {
        let slug_cf = self.cf("schema_prop_slug")?;
        let prop_cf = self.cf("schema_props")?;

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let slug_key = encode_branch_slug_key(&ancestor_id, slug);
            if let Some(id_bytes) = self.db.get_cf(slug_cf, &slug_key)
                .map_err(|e| SchemaError::Storage(e.to_string()))? {
                let prop_id = Uuid::from_slice(&id_bytes)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;
                let prefix = SchemaPropertyKey::prefix_branch_prop(&ancestor_id, &prop_id);
                let iter = self.db.prefix_iterator_cf(prop_cf, &prefix);
                for item in iter {
                    let (raw_key, val_bytes) = item.map_err(|e| SchemaError::Storage(e.to_string()))?;
                    if !raw_key.starts_with(&prefix) { break; }
                    let k = SchemaPropertyKey::decode(&raw_key)
                        .map_err(|e| SchemaError::Storage(e.to_string()))?;
                    if k.tx_id.as_bytes() <= branch_point_tx.as_bytes() {
                        let pv: PropertyValue = serde_json::from_slice(&val_bytes)
                            .map_err(|e| SchemaError::Storage(e.to_string()))?;
                        let created_at = parse_datetime(&pv.created_at)?;
                        let value_type = ValueType::from_str(&pv.value_type)
                            .ok_or_else(|| SchemaError::Storage(format!("invalid value_type: {}", pv.value_type)))?;
                        return Ok(EntityProperty {
                            id: prop_id,
                            slug: pv.slug,
                            description: pv.description,
                            value_type,
                            created_at,
                        });
                    }
                }
            }
        }

        Err(SchemaError::PropertyNotFound(slug.to_string()))
    }

    /// Lists properties attached to a given entity type (chain walk for attachments).
    pub fn list_properties(&self, entity_type_slug: &str) -> Result<Vec<EntityProperty>, SchemaError> {
        let et = self.get_entity_type(entity_type_slug)?;
        self.list_properties_by_type_id(&et.id)
    }

    /// Lists properties by entity type UUID (chain walk for attachments).
    pub fn list_properties_by_type_id(&self, entity_type_id: &Uuid) -> Result<Vec<EntityProperty>, SchemaError> {
        // Verify entity type exists
        self.get_entity_type_by_id(entity_type_id)?;

        let attach_cf = self.cf("schema_attachments")?;
        let mut prop_ids = std::collections::HashSet::new();

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let prefix = SchemaAttachmentKey::prefix_branch_type(&ancestor_id, entity_type_id);
            let iter = self.db.prefix_iterator_cf(attach_cf, &prefix);
            for item in iter {
                let (raw_key, _) = item.map_err(|e| SchemaError::Storage(e.to_string()))?;
                if !raw_key.starts_with(&prefix) { break; }

                let k = SchemaAttachmentKey::decode(&raw_key)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;
                if k.tx_id.as_bytes() <= branch_point_tx.as_bytes() {
                    prop_ids.insert(k.prop_id);
                }
            }
        }

        let mut props = Vec::with_capacity(prop_ids.len());
        for prop_id in prop_ids {
            if let Ok(prop) = self.get_property_by_id(&prop_id) {
                props.push(prop);
            }
        }
        props.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(props)
    }

    /// Gets a property by UUID (chain walk).
    fn get_property_by_id(&self, id: &Uuid) -> Result<EntityProperty, SchemaError> {
        let prop_cf = self.cf("schema_props")?;

        for &(ancestor_id, branch_point_tx) in &self.ancestry {
            let prefix = SchemaPropertyKey::prefix_branch_prop(&ancestor_id, id);
            let iter = self.db.prefix_iterator_cf(prop_cf, &prefix);
            for item in iter {
                let (raw_key, val_bytes) = item.map_err(|e| SchemaError::Storage(e.to_string()))?;
                if !raw_key.starts_with(&prefix) { break; }
                let k = SchemaPropertyKey::decode(&raw_key)
                    .map_err(|e| SchemaError::Storage(e.to_string()))?;
                if k.tx_id.as_bytes() <= branch_point_tx.as_bytes() {
                    let pv: PropertyValue = serde_json::from_slice(&val_bytes)
                        .map_err(|e| SchemaError::Storage(e.to_string()))?;
                    let created_at = parse_datetime(&pv.created_at)?;
                    let value_type = ValueType::from_str(&pv.value_type)
                        .ok_or_else(|| SchemaError::Storage(format!("invalid value_type: {}", pv.value_type)))?;
                    return Ok(EntityProperty {
                        id: *id,
                        slug: pv.slug,
                        description: pv.description,
                        value_type,
                        created_at,
                    });
                }
            }
        }

        Err(SchemaError::PropertyNotFound(id.to_string()))
    }

    // -- Attachments --

    /// Attaches a property to an entity type.
    pub fn attach_property(
        &self,
        entity_type_slug: &str,
        property_slug: &str,
    ) -> Result<(), SchemaError> {
        let et = self.get_entity_type(entity_type_slug)?;
        let prop = self.get_property_by_slug(property_slug)?;

        let cf = self.cf("schema_attachments")?;
        let tx_id = Uuid::now_v7();
        let key = SchemaAttachmentKey {
            branch_id: self.branch_id,
            type_id: et.id,
            tx_id,
            prop_id: prop.id,
        }.encode();

        self.db.put_cf(cf, &key, &[])
            .map_err(|e| SchemaError::Storage(e.to_string()))
    }

    /// Attaches properties in batch.
    pub fn attach_properties_batch(
        &self,
        items: &[AttachInput<'_>],
    ) -> Result<usize, SchemaError> {
        // Validate all first
        let mut resolved = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let et = self.get_entity_type(item.entity_type).map_err(|e| {
                SchemaError::BatchItemError {
                    index: i,
                    source: Box::new(e),
                }
            })?;
            let prop = self.get_property_by_slug(item.property).map_err(|e| {
                SchemaError::BatchItemError {
                    index: i,
                    source: Box::new(e),
                }
            })?;
            resolved.push((et.id, prop.id));
        }

        let cf = self.cf("schema_attachments")?;
        let mut batch = rocksdb::WriteBatch::default();

        for (type_id, prop_id) in resolved {
            let tx_id = Uuid::now_v7();
            let key = SchemaAttachmentKey {
                branch_id: self.branch_id,
                type_id,
                tx_id,
                prop_id,
            }.encode();
            batch.put_cf(cf, &key, &[]);
        }

        self.db.write(batch).map_err(|e| SchemaError::Storage(e.to_string()))?;
        Ok(items.len())
    }

    // -- Private helpers --

    fn write_entity_type(&self, id: Uuid, slug: &str, description: Option<&str>, created_at: &DateTime<Utc>) -> Result<(), SchemaError> {
        let cf = self.cf("schema_types")?;
        let tx_id = Uuid::now_v7();
        let key = SchemaTypeKey { branch_id: self.branch_id, type_id: id, tx_id }.encode();
        let val = EntityTypeValue {
            slug: slug.to_string(),
            description: description.map(String::from),
            created_at: created_at.to_rfc3339(),
        };
        let val_bytes = serde_json::to_vec(&val)
            .map_err(|e| SchemaError::Storage(e.to_string()))?;
        self.db.put_cf(cf, &key, &val_bytes)
            .map_err(|e| SchemaError::Storage(e.to_string()))
    }

    fn write_type_slug_index(&self, slug: &str, id: &Uuid) -> Result<(), SchemaError> {
        let cf = self.cf("schema_type_slug")?;
        let key = encode_branch_slug_key(&self.branch_id, slug);
        self.db.put_cf(cf, &key, id.as_bytes())
            .map_err(|e| SchemaError::Storage(e.to_string()))
    }

    fn write_property(&self, id: Uuid, slug: &str, value_type: ValueType, description: Option<&str>, created_at: &DateTime<Utc>) -> Result<(), SchemaError> {
        let cf = self.cf("schema_props")?;
        let tx_id = Uuid::now_v7();
        let key = SchemaPropertyKey { branch_id: self.branch_id, prop_id: id, tx_id }.encode();
        let val = PropertyValue {
            slug: slug.to_string(),
            description: description.map(String::from),
            value_type: value_type.as_str().to_string(),
            created_at: created_at.to_rfc3339(),
        };
        let val_bytes = serde_json::to_vec(&val)
            .map_err(|e| SchemaError::Storage(e.to_string()))?;
        self.db.put_cf(cf, &key, &val_bytes)
            .map_err(|e| SchemaError::Storage(e.to_string()))
    }

    fn write_prop_slug_index(&self, slug: &str, id: &Uuid) -> Result<(), SchemaError> {
        let cf = self.cf("schema_prop_slug")?;
        let key = encode_branch_slug_key(&self.branch_id, slug);
        self.db.put_cf(cf, &key, id.as_bytes())
            .map_err(|e| SchemaError::Storage(e.to_string()))
    }

    fn cf(&self, name: &str) -> Result<&rocksdb::ColumnFamily, SchemaError> {
        self.db
            .cf_handle(name)
            .ok_or_else(|| SchemaError::Storage(format!("missing column family: {name}")))
    }
}

fn encode_branch_slug_key(branch_id: &Uuid, slug: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(16 + slug.len());
    key.extend_from_slice(branch_id.as_bytes());
    key.extend_from_slice(slug.as_bytes());
    key
}

fn parse_datetime(s: &str) -> Result<DateTime<Utc>, SchemaError> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| SchemaError::Storage(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::AssertionStore;
    use crate::assertion::MAIN_BRANCH;

    fn setup() -> (AssertionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (store, dir)
    }

    fn main_registry(store: &AssertionStore) -> BranchSchemaRegistry {
        store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())])
    }

    #[test]
    fn create_entity_type() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        let et = reg.create_entity_type("research-project", None).unwrap();
        assert_eq!(et.slug, "research-project");
    }

    #[test]
    fn reject_duplicate_entity_type() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        reg.create_entity_type("unit", None).unwrap();
        assert!(matches!(
            reg.create_entity_type("unit", None),
            Err(SchemaError::DuplicateEntityType(_))
        ));
    }

    #[test]
    fn create_property_and_attach() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        reg.create_entity_type("research-project", None).unwrap();
        let prop = reg.create_property("project-name", ValueType::Struct, None).unwrap();
        assert_eq!(prop.slug, "project-name");

        reg.attach_property("research-project", "project-name").unwrap();

        let props = reg.list_properties("research-project").unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].slug, "project-name");
    }

    #[test]
    fn list_entity_types() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        reg.create_entity_type("bravo", None).unwrap();
        reg.create_entity_type("alpha", None).unwrap();

        let types = reg.list_entity_types().unwrap();
        assert_eq!(types.len(), 2);
        assert_eq!(types[0].slug, "alpha");
        assert_eq!(types[1].slug, "bravo");
    }

    #[test]
    fn get_entity_type() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        let created = reg.create_entity_type("sensor", None).unwrap();
        let fetched = reg.get_entity_type("sensor").unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[test]
    fn reject_reserved_property() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        for slug in &["entity-uuid", "entity-name", "entity-type"] {
            assert!(matches!(
                reg.create_property(slug, ValueType::Struct, None),
                Err(SchemaError::ReservedProperty(_))
            ));
        }
    }

    #[test]
    fn batch_create_entity_types() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        let items = vec![
            EntityTypeInput { slug: "alpha", description: None, properties: &[] },
            EntityTypeInput { slug: "bravo", description: Some("Second"), properties: &[] },
        ];
        let results = reg.create_entity_types_batch(&items).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].slug, "alpha");
    }

    #[test]
    fn batch_create_properties() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        let items = vec![
            PropertyInput { slug: "name", value_type: ValueType::Struct, description: None, entity_types: &[] },
            PropertyInput { slug: "count", value_type: ValueType::Range, description: Some("Amount"), entity_types: &[] },
        ];
        let results = reg.create_properties_batch(&items).unwrap();
        assert_eq!(results.len(), 2);

        let all = reg.list_all_properties().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn branch_isolation() {
        let (store, _dir) = setup();

        // Create entity type on main
        let main_reg = main_registry(&store);
        main_reg.create_entity_type("main-type", None).unwrap();
        main_reg.create_property("main-prop", ValueType::Set, None).unwrap();

        // Create a branch
        let branches = store.branches();
        let branch = branches.create("child", serde_json::Value::Null, MAIN_BRANCH, Uuid::max()).unwrap();
        let ancestry = branches.resolve_ancestry(&branch.id).unwrap();
        let branch_reg = store.schema_registry(branch.id, ancestry);

        // Branch can see main's types
        assert!(branch_reg.get_entity_type("main-type").is_ok());

        // Create type only on branch
        branch_reg.create_entity_type("branch-type", None).unwrap();

        // Main cannot see branch's types
        let main_types = main_reg.list_entity_types().unwrap();
        assert_eq!(main_types.len(), 1);
        assert_eq!(main_types[0].slug, "main-type");

        // Branch can see both
        let branch_types = branch_reg.list_entity_types().unwrap();
        assert_eq!(branch_types.len(), 2);
    }

    #[test]
    fn batch_create_entity_types_with_property_attachment() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        reg.create_property("name", ValueType::Struct, None).unwrap();
        reg.create_property("code", ValueType::Range, None).unwrap();

        let items = vec![
            EntityTypeInput { slug: "unit", description: None, properties: &["name", "code"] },
            EntityTypeInput { slug: "location", description: None, properties: &["name"] },
        ];
        let results = reg.create_entity_types_batch(&items).unwrap();
        assert_eq!(results.len(), 2);

        let unit_props = reg.list_properties("unit").unwrap();
        assert_eq!(unit_props.len(), 2);

        let loc_props = reg.list_properties("location").unwrap();
        assert_eq!(loc_props.len(), 1);
    }

    #[test]
    fn batch_attach_properties() {
        let (store, _dir) = setup();
        let reg = main_registry(&store);

        reg.create_entity_type("unit", None).unwrap();
        reg.create_property("name", ValueType::Struct, None).unwrap();
        reg.create_property("code", ValueType::Range, None).unwrap();

        let items = vec![
            AttachInput { entity_type: "unit", property: "name" },
            AttachInput { entity_type: "unit", property: "code" },
        ];
        let count = reg.attach_properties_batch(&items).unwrap();
        assert_eq!(count, 2);

        let props = reg.list_properties("unit").unwrap();
        assert_eq!(props.len(), 2);
    }
}
