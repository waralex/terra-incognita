//! ExecuteTransaction — validates and writes a transaction to a branch.

use std::collections::HashSet;

use uuid::Uuid;

use crate::command::Command;
use crate::command::CommandState;
use crate::command::input::transaction::{DeleteItem, TransactionInput};
use crate::domain::entity::Entity;
use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::{TxMeta, time_from_uuid};
use crate::domain::validator::DomainValidator;
use crate::io::DbError;
use crate::io::slug::Slug;
use crate::io::WriteBatch;
use crate::io::storage_key::StorageKey;
use crate::store::branch_context::BranchContext;
use crate::store::entry::assertion::{AssertionEntry, AssertionKey, AssertionValue};
use crate::store::entry::embedding::{EmbeddingEntry, EmbeddingKey, EmbeddingValue};
use crate::store::entry::entity::{EntityEntry, EntityKey, EntityValue};
use crate::store::entry::entity_change::{EntityChangeEntry, EntityChangeKey, EntityChangeValue};
use crate::store::entry::managed::{ManagedEntry, ManagedKey, ManagedValue};
use crate::store::entry::touched::{TouchedEntry, TouchedKey, TouchedValue};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey, TransactionValue};
use crate::store::entry::transaction_log::{TransactionLogEntry, TransactionLogKey, TransactionLogValue, ChangeItem, ManagedItem};
use crate::store::query::properties;

/// Validates and writes a transaction to a branch.
pub struct ExecuteTransaction {
    validator: DomainValidator,
}

impl ExecuteTransaction {
    /// Create an executor with the given validator.
    pub fn new(validator: DomainValidator) -> Self {
        Self { validator }
    }
}

impl ExecuteTransaction {
    fn write_touched(
        branch: &BranchContext,
        batch: &mut WriteBatch,
        tx_id: Uuid,
        entity: &Entity,
    ) -> Result<(), DbError> {
        let reasoning = entity.meta.get("reasoning")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        batch.put(&TouchedEntry {
            key: TouchedKey {
                branch: branch.id().clone(),
                tx_id,
                entity: entity.slug.clone(),
            },
            value: TouchedValue { reasoning },
        })
    }

    fn write_assertions(
        &self,
        branch: &BranchContext,
        state: &mut CommandState,
        tx_id: Uuid,
        entity: &Entity,
    ) -> Result<Uuid, DbError> {
        let change_id = Uuid::now_v7();

        if !entity.properties.is_empty() {
            let reasoning = entity.meta.get("reasoning")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let batch = state.batch();
            batch.put(&EntityChangeEntry {
                key: EntityChangeKey { change_id },
                value: EntityChangeValue {
                    entity: entity.slug.clone(),
                    tx_id,
                    meta: entity.meta.clone(),
                },
            })?;

            for pv in &entity.properties {
                batch.put(&AssertionEntry {
                    key: AssertionKey {
                        branch: branch.id().clone(),
                        entity: entity.slug.clone(),
                        prop: pv.property.clone(),
                        tx_id,
                    },
                    value: AssertionValue {
                        change_id,
                        value: pv.value.clone(),
                        reasoning: reasoning.clone(),
                    },
                })?;
            }
        }

        Ok(change_id)
    }

    /// Build a text representation of an entity for embedding.
    ///
    /// Pure function — all data is provided by the caller.
    /// `description` is the resolved description (from input or from storage).
    /// `existing` contains previously-stored properties not overridden by the input.
    fn build_embed_text(
        entity: &Entity,
        description: Option<&serde_json::Value>,
        existing: &[AssertionEntry],
    ) -> String {
        let mut lines = Vec::new();

        lines.push(format!("entity: {}", entity.slug));

        if let Some(desc) = description {
            lines.push(format!("description: {}", desc));
        }

        let input_props: std::collections::HashSet<&crate::io::slug::Slug> =
            entity.properties.iter().map(|pv| &pv.property).collect();

        for pv in &entity.properties {
            lines.push(format!("{}: {}", pv.property, pv.value));
        }

        for a in existing {
            if !input_props.contains(&a.key.prop) {
                lines.push(format!("{}: {}", a.key.prop, a.value.value));
            }
        }

        lines.join("\n")
    }

    /// Generate and write embedding for an entity if the embedder is active.
    fn write_embedding(
        state: &mut CommandState,
        branch: &BranchContext,
        tx_id: Uuid,
        entity: &Entity,
        change_id: Uuid,
        description: Option<&serde_json::Value>,
        existing: &[AssertionEntry],
    ) -> Result<(), DbError> {
        if state.embedder().dimensions() == 0 {
            return Ok(());
        }

        let text = Self::build_embed_text(entity, description, existing);
        let embedding = state.embedder().embed(&text)?;

        if embedding.is_empty() {
            return Ok(());
        }

        state.batch().put(&EmbeddingEntry {
            key: EmbeddingKey {
                branch: branch.id().clone(),
                entity: entity.slug.clone(),
                tx_id,
            },
            value: EmbeddingValue {
                change_id,
                embedding,
            },
        })?;

        Ok(())
    }

    fn create_entity(
        &self,
        branch: &BranchContext,
        state: &mut CommandState,
        tx_id: Uuid,
        entity: &Entity,
    ) -> Result<ChangeItem, DbError> {
        let bound = EntityKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = entity.slug.clone(); });
        if branch.exists::<EntityEntry>(&bound)? {
            return Err(DbError::Storage(format!(
                "entity already exists: {}", entity.slug
            )));
        }

        state.batch().put(&EntityEntry {
            key: EntityKey {
                branch: branch.id().clone(),
                entity: entity.slug.clone(),
                tx_id,
            },
            value: EntityValue {
                description: entity.description.clone(),
                ..Default::default()
            },
        })?;

        Self::write_touched(branch, state.batch(), tx_id, entity)?;
        let change_id = self.write_assertions(branch, state, tx_id, entity)?;
        // New entity — description from input, no existing properties.
        Self::write_embedding(state, branch, tx_id, entity, change_id, entity.description.as_ref(), &[])?;

        Ok(ChangeItem {
            entity: entity.slug.clone(),
            change_id,
            properties: entity.properties.iter().map(|pv| pv.property.clone()).collect(),
        })
    }

    fn update_entity(
        &self,
        branch: &BranchContext,
        state: &mut CommandState,
        tx_id: Uuid,
        entity: &Entity,
    ) -> Result<ChangeItem, DbError> {
        let bound = EntityKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = entity.slug.clone(); });
        if !branch.exists::<EntityEntry>(&bound)? {
            return Err(DbError::Storage(format!(
                "entity not found: {}", entity.slug
            )));
        }

        // Read existing state for embedding BEFORE writing to batch.
        // Guarded: skip DB reads when embedder is inactive.
        let stored_desc: Option<serde_json::Value>;
        let existing: Vec<AssertionEntry>;
        if state.embedder().dimensions() != 0 {
            stored_desc = if entity.description.is_none() {
                branch.get_latest::<EntityEntry>(&bound)?
                    .and_then(|e| e.value.description)
            } else {
                None
            };
            existing = properties::properties(branch, &entity.slug, None)?;
        } else {
            stored_desc = None;
            existing = vec![];
        }
        let description = entity.description.as_ref().or(stored_desc.as_ref());

        if entity.description.is_some() {
            state.batch().put(&EntityEntry {
                key: EntityKey {
                    branch: branch.id().clone(),
                    entity: entity.slug.clone(),
                    tx_id,
                },
                value: EntityValue {
                    description: entity.description.clone(),
                    ..Default::default()
                },
            })?;
        }

        Self::write_touched(branch, state.batch(), tx_id, entity)?;
        let change_id = self.write_assertions(branch, state, tx_id, entity)?;
        Self::write_embedding(state, branch, tx_id, entity, change_id, description, &existing)?;

        Ok(ChangeItem {
            entity: entity.slug.clone(),
            change_id,
            properties: entity.properties.iter().map(|pv| pv.property.clone()).collect(),
        })
    }

    fn create_managed_item(
        &self,
        branch: &BranchContext,
        batch: &mut WriteBatch,
        tx_id: Uuid,
        managed: &crate::domain::managed::Managed,
    ) -> Result<(), DbError> {
        let bound = ManagedKey::bound()
            .with_prefix(|k| {
                k.branch = branch.id().clone();
                k.type_name = managed.type_name.clone();
                k.item = managed.slug.clone();
            });
        if branch.exists::<ManagedEntry>(&bound)? {
            return Err(DbError::Storage(format!(
                "managed item already exists: {}/{}", managed.type_name, managed.slug
            )));
        }

        batch.put(&ManagedEntry {
            key: ManagedKey {
                branch: branch.id().clone(),
                type_name: managed.type_name.clone(),
                item: managed.slug.clone(),
                tx_id,
            },
            value: ManagedValue {
                slug: managed.slug.to_string(),
                state: managed.state.clone(),
                fields: managed.fields.clone(),
            },
        })?;

        Ok(())
    }

    fn update_managed_item(
        &self,
        branch: &BranchContext,
        batch: &mut WriteBatch,
        tx_id: Uuid,
        managed: &crate::domain::managed::Managed,
    ) -> Result<(), DbError> {
        let bound = ManagedKey::bound()
            .with_prefix(|k| {
                k.branch = branch.id().clone();
                k.type_name = managed.type_name.clone();
                k.item = managed.slug.clone();
            });
        let existing = branch.get_latest::<ManagedEntry>(&bound)?
            .ok_or_else(|| DbError::Storage(format!(
                "managed item not found: {}/{}", managed.type_name, managed.slug
            )))?;

        // Merge fields: start with existing, overlay with input.
        // Null values in input remove the field.
        let mut merged_fields = existing.value.fields;
        for (key, value) in &managed.fields {
            if value.is_null() {
                merged_fields.remove(key);
            } else {
                merged_fields.insert(key.clone(), value.clone());
            }
        }

        // Carry forward existing state when input state is None.
        let merged_state = managed.state.clone().or(existing.value.state);

        batch.put(&ManagedEntry {
            key: ManagedKey {
                branch: branch.id().clone(),
                type_name: managed.type_name.clone(),
                item: managed.slug.clone(),
                tx_id,
            },
            value: ManagedValue {
                slug: managed.slug.to_string(),
                state: merged_state,
                fields: merged_fields,
            },
        })?;

        Ok(())
    }

    fn delete_entity(
        &self,
        branch: &BranchContext,
        state: &mut CommandState,
        tx_id: Uuid,
        item: &DeleteItem,
    ) -> Result<ChangeItem, DbError> {
        let bound = EntityKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = item.entity.clone(); });
        if !branch.exists::<EntityEntry>(&bound)? {
            return Err(DbError::Storage(format!(
                "entity not found: {}", item.entity
            )));
        }

        state.batch().put(&EntityEntry {
            key: EntityKey {
                branch: branch.id().clone(),
                entity: item.entity.clone(),
                tx_id,
            },
            value: EntityValue {
                deleted: Some(item.reasoning.clone()),
                ..Default::default()
            },
        })?;

        // Write EntityChangeEntry for provenance.
        let change_id = Uuid::now_v7();
        let mut meta = serde_json::Map::new();
        meta.insert("reasoning".into(), item.reasoning.clone());
        state.batch().put(&EntityChangeEntry {
            key: EntityChangeKey { change_id },
            value: EntityChangeValue {
                entity: item.entity.clone(),
                tx_id,
                meta,
            },
        })?;

        // Deactivate embedding.
        state.batch().put(&EmbeddingEntry {
            key: EmbeddingKey {
                branch: branch.id().clone(),
                entity: item.entity.clone(),
                tx_id,
            },
            value: EmbeddingValue {
                change_id: Uuid::nil(),
                embedding: vec![],
            },
        })?;

        Self::write_touched(branch, state.batch(), tx_id, &Entity::new(
            item.entity.clone(),
            None,
            vec![],
            serde_json::Map::new(),
        ))?;

        Ok(ChangeItem {
            entity: item.entity.clone(),
            change_id,
            properties: vec![],
        })
    }
}

impl Command for ExecuteTransaction {
    type Input = TransactionInput;
    type Output = Transaction<TxMeta>;

    fn execute(&self, branch: &BranchContext, state: &mut CommandState, input: Self::Input) -> Result<Self::Output, DbError> {
        // Validate everything before touching storage.
        self.validator.check_transaction(&Transaction::new(input.meta.clone()))?;
        for entity in &input.create_entities {
            self.validator.check_entity_create(entity)?;
        }
        for entity in &input.update_entities {
            self.validator.check_entity_update(entity)?;
        }
        for managed in &input.create_managed {
            self.validator.check_managed_create(managed)?;
        }
        for managed in &input.update_managed {
            self.validator.check_managed_update(managed)?;
        }

        let tx_id = Uuid::now_v7();

        let mut created_entity_slugs: HashSet<&Slug> = HashSet::new();
        let mut created_items = Vec::new();
        for entity in &input.create_entities {
            if !created_entity_slugs.insert(&entity.slug) {
                return Err(DbError::Storage(format!(
                    "duplicate entity in transaction: {}", entity.slug
                )));
            }
            created_items.push(self.create_entity(branch, state, tx_id, entity)?);
        }

        let mut updated_items = Vec::new();
        for entity in &input.update_entities {
            updated_items.push(self.update_entity(branch, state, tx_id, entity)?);
        }

        let mut deleted_items = Vec::new();
        for item in &input.delete_entities {
            deleted_items.push(self.delete_entity(branch, state, tx_id, item)?);
        }

        let mut created_managed_slugs: HashSet<(&Slug, &Slug)> = HashSet::new();
        let mut created_managed_items = Vec::new();
        for managed in &input.create_managed {
            if !created_managed_slugs.insert((&managed.type_name, &managed.slug)) {
                return Err(DbError::Storage(format!(
                    "duplicate managed item in transaction: {}/{}", managed.type_name, managed.slug
                )));
            }
            self.create_managed_item(branch, state.batch(), tx_id, managed)?;
            created_managed_items.push(ManagedItem {
                type_name: managed.type_name.clone(),
                slug: managed.slug.clone(),
            });
        }

        let mut updated_managed_items = Vec::new();
        for managed in &input.update_managed {
            self.update_managed_item(branch, state.batch(), tx_id, managed)?;
            updated_managed_items.push(ManagedItem {
                type_name: managed.type_name.clone(),
                slug: managed.slug.clone(),
            });
        }

        // Explicit touches — applied last to override auto-touches.
        let mut touched_slugs = Vec::new();
        for item in &input.touched {
            state.batch().put(&TouchedEntry {
                key: TouchedKey {
                    branch: branch.id().clone(),
                    tx_id,
                    entity: item.entity.clone(),
                },
                value: TouchedValue { reasoning: item.reasoning.clone() },
            })?;
            touched_slugs.push(item.entity.clone());
        }

        state.batch().put(&TransactionLogEntry {
            key: TransactionLogKey { tx_id },
            value: TransactionLogValue {
                branch: branch.id().clone(),
                created: created_items,
                updated: updated_items,
                touched: touched_slugs,
                deleted: deleted_items,
                created_managed: created_managed_items,
                updated_managed: updated_managed_items,
            },
        })?;

        state.batch().put(&TransactionEntry {
            key: TransactionKey {
                branch: branch.id().clone(),
                tx_id,
            },
            value: TransactionValue { meta: input.meta.clone() },
        })?;

        Ok(Transaction {
            meta: input.meta,
            context: TxMeta {
                tx_id,
                branch: branch.id().clone(),
                reasoning: None,
                time: time_from_uuid(tx_id),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use super::*;
    use crate::config::ProjectConfig;
    use crate::domain::entity::PropertyValue;
    use crate::domain::managed::Managed;
    use crate::store::query::similarity;
    use crate::store::storage::Storage;
    use serde_json::{Map, Value};

    use indoc::indoc;
    use crate::command::input::transaction::TouchItem;
    use crate::config::DataSchema;

    fn test_config() -> Arc<ProjectConfig> {
        Arc::new(ProjectConfig::builder()
            .data_dir("./data".into())
            .schema_path("./schema.yaml".into())
            .build())
    }

    fn test_schema() -> Arc<DataSchema> {
        Arc::new(DataSchema::from_yaml(indoc! {"
            transaction_meta:
              reasoning:
                type: text
                required: true
            entity_change_meta:
              reasoning:
                type: text
                required: true
            managed_types:
              task:
                fields:
                  goal: { type: json, required: true }
                  notes: { type: json }
                lifecycle:
                  initial: open
                  states: [open, closed]
                  visible: [open]
        "}).unwrap())
    }

    fn cmd() -> ExecuteTransaction {
        ExecuteTransaction::new(DomainValidator::new(test_schema()))
    }

    fn meta(reasoning: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(reasoning.into()));
        m
    }

    fn entity_meta(reasoning: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(reasoning.into()));
        m
    }

    fn entity_bound(branch: &BranchContext, slug: &str) -> crate::io::KeyBound<EntityKey> {
        EntityKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = slug.parse().unwrap(); })
    }

    fn managed_bound(branch: &BranchContext, type_name: &str, slug: &str) -> crate::io::KeyBound<ManagedKey> {
        ManagedKey::bound()
            .with_prefix(|k| {
                k.branch = branch.id().clone();
                k.type_name = type_name.parse().unwrap();
                k.item = slug.parse().unwrap();
            })
    }

    /// Execute a transaction and commit immediately (test convenience).
    fn exec(branch: &BranchContext, input: TransactionInput) -> Transaction<TxMeta> {
        let cmd = cmd();
        let mut state = CommandState::new(branch.storage());
        let result = cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
        result
    }

    // --- Create entity ---

    #[test]
    fn create_single_entity() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("introduce alice"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            )));

        assert_eq!(result.meta["reasoning"], "introduce alice");
        assert_eq!(result.context.branch.as_str(), "main");

        let entry = branch.get_latest::<EntityEntry>(&entity_bound(&branch, "alice")).unwrap().unwrap();
        assert_eq!(entry.key.entity.as_str(), "alice");
        assert_eq!(entry.value.description, Some(serde_json::json!("A person")));
    }

    #[test]
    fn duplicate_entity_slug_fails() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(&branch, TransactionInput::new(meta("first"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("First")),
                vec![],
                Map::new(),
            )));

        let cmd = cmd();
        let mut state = CommandState::new(branch.storage());
        let err = cmd.execute(&branch, &mut state, TransactionInput::new(meta("second"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Duplicate")),
                vec![],
                Map::new(),
            ))
        ).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn empty_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("no-op")));
        assert_eq!(result.meta["reasoning"], "no-op");
    }

    #[test]
    fn multiple_entities_in_one_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("batch"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Person A")),
                vec![],
                Map::new(),
            ))
            .create_entity(Entity::new(
                "bob".parse().unwrap(),
                Some(serde_json::json!("Person B")),
                vec![],
                Map::new(),
            )));

        for name in ["alice", "bob"] {
            let entry = branch.get_latest::<EntityEntry>(&entity_bound(&branch, name)).unwrap().unwrap();
            assert_eq!(entry.key.entity.as_str(), name);
            assert_eq!(entry.key.tx_id, result.context.tx_id);
        }
    }

    #[test]
    fn entity_exists_after_create() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let prefix = entity_bound(&branch, "alice");
        assert!(!branch.exists::<EntityEntry>(&prefix).unwrap());

        exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            )));

        assert!(branch.exists::<EntityEntry>(&prefix).unwrap());
    }

    #[test]
    fn create_entity_with_initial_properties() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("create with props"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                    PropertyValue { property: "city".parse().unwrap(), value: serde_json::json!("London"), context: () },
                ],
                entity_meta("initial observation"),
            )));

        let entry = branch.get_latest::<EntityEntry>(&entity_bound(&branch, "alice")).unwrap().unwrap();
        assert_eq!(entry.value.description, Some(serde_json::json!("A person")));

        let age_slug: crate::io::Slug = "age".parse().unwrap();
        let alice_slug: crate::io::Slug = "alice".parse().unwrap();
        let bound = AssertionKey::bound()
            .with_prefix(|k| {
                k.branch = branch.id().clone();
                k.entity = alice_slug.clone();
                k.prop = age_slug.clone();
            });
        let found = branch.storage().get_latest::<AssertionEntry>(&bound).unwrap().unwrap();
        assert_eq!(found.value.value, serde_json::json!(30));
        assert_eq!(found.key.entity, alice_slug);
        assert_eq!(found.key.tx_id, result.context.tx_id);

        let change = storage_get_exact(&branch, found.value.change_id).unwrap();
        assert_eq!(change.value.meta["reasoning"], "initial observation");
        assert_eq!(change.value.entity, alice_slug.as_str());
    }

    fn storage_get_exact(branch: &BranchContext, change_id: Uuid) -> Result<EntityChangeEntry, DbError> {
        let key = EntityChangeKey { change_id };
        branch.storage().get::<EntityChangeEntry>(&key)?
            .ok_or_else(|| DbError::Storage("entity change not found".into()))
    }

    // --- Update entity ---

    #[test]
    fn update_entity_writes_assertions() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            )));

        let result = exec(&branch, TransactionInput::new(meta("update"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![
                    PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                ],
                entity_meta("age observed"),
            )));

        let age_slug: crate::io::Slug = "age".parse().unwrap();
        let alice_slug: crate::io::Slug = "alice".parse().unwrap();
        let bound = AssertionKey::bound()
            .with_prefix(|k| {
                k.branch = branch.id().clone();
                k.entity = alice_slug.clone();
                k.prop = age_slug.clone();
            });
        let found = branch.storage().get_latest::<AssertionEntry>(&bound).unwrap().unwrap();
        assert_eq!(found.value.value, serde_json::json!(25));
        assert_eq!(found.key.tx_id, result.context.tx_id);
    }

    #[test]
    fn update_entity_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let cmd = cmd();
        let mut state = CommandState::new(branch.storage());
        let err = cmd.execute(&branch, &mut state, TransactionInput::new(meta("update missing"))
            .update_entity(Entity::new(
                "ghost".parse().unwrap(),
                None,
                vec![],
                Map::new(),
            ))
        ).unwrap_err();
        assert!(err.to_string().contains("entity not found: ghost"));
    }

    #[test]
    fn update_entity_writes_description() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Original")),
                vec![],
                Map::new(),
            )));

        exec(&branch, TransactionInput::new(meta("update desc"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Updated description")),
                vec![],
                Map::new(),
            )));

        let entry = branch.get_latest::<EntityEntry>(&entity_bound(&branch, "alice")).unwrap().unwrap();
        assert_eq!(entry.value.description, Some(serde_json::json!("Updated description")));
    }

    // --- Create managed ---

    #[test]
    fn create_managed_item() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("explore"));

        exec(&branch, TransactionInput::new(meta("create task"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            )));

        let entry = branch.get_latest::<ManagedEntry>(&managed_bound(&branch, "task", "task-1"))
            .unwrap().unwrap();
        assert_eq!(entry.value.slug, "task-1");
        assert_eq!(entry.value.state, Some("open".into()));
        assert_eq!(entry.value.fields["goal"], "explore");
    }

    #[test]
    fn create_managed_duplicate_fails() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("first"));

        exec(&branch, TransactionInput::new(meta("create"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields.clone(),
            )));

        let cmd = cmd();
        let mut state = CommandState::new(branch.storage());
        let err = cmd.execute(&branch, &mut state, TransactionInput::new(meta("duplicate"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            ))
        ).unwrap_err();
        assert!(err.to_string().contains("managed item already exists: task/task-1"));
    }

    // --- Update managed ---

    #[test]
    fn update_managed_item() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("explore"));

        exec(&branch, TransactionInput::new(meta("create"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            )));

        let mut updated_fields = Map::new();
        updated_fields.insert("goal".into(), serde_json::json!("explore deeply"));
        updated_fields.insert("notes".into(), serde_json::json!("found something"));

        exec(&branch, TransactionInput::new(meta("update"))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                updated_fields,
            )));

        let entry = branch.get_latest::<ManagedEntry>(&managed_bound(&branch, "task", "task-1"))
            .unwrap().unwrap();
        assert_eq!(entry.value.fields["goal"], "explore deeply");
        assert_eq!(entry.value.fields["notes"], "found something");
    }

    #[test]
    fn update_managed_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let cmd = cmd();
        let mut state = CommandState::new(branch.storage());
        let err = cmd.execute(&branch, &mut state, TransactionInput::new(meta("update missing"))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "ghost".parse().unwrap(),
                Some("open".into()),
                Map::new(),
            ))
        ).unwrap_err();
        assert!(err.to_string().contains("managed item not found: task/ghost"));
    }

    #[test]
    fn update_managed_partial_fields_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("explore"));
        fields.insert("notes".into(), serde_json::json!("initial notes"));

        exec(&branch, TransactionInput::new(meta("create"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            )));

        // Update only notes — goal should be preserved.
        let mut update_fields = Map::new();
        update_fields.insert("notes".into(), serde_json::json!("updated notes"));

        exec(&branch, TransactionInput::new(meta("partial update"))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                update_fields,
            )));

        let entry = branch.get_latest::<ManagedEntry>(&managed_bound(&branch, "task", "task-1"))
            .unwrap().unwrap();
        assert_eq!(entry.value.fields["goal"], "explore");
        assert_eq!(entry.value.fields["notes"], "updated notes");
    }

    #[test]
    fn update_managed_null_removes_field() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("explore"));
        fields.insert("notes".into(), serde_json::json!("some notes"));

        exec(&branch, TransactionInput::new(meta("create"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            )));

        // Send null for notes — should be removed.
        let mut update_fields = Map::new();
        update_fields.insert("notes".into(), serde_json::Value::Null);

        exec(&branch, TransactionInput::new(meta("remove notes"))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                update_fields,
            )));

        let entry = branch.get_latest::<ManagedEntry>(&managed_bound(&branch, "task", "task-1"))
            .unwrap().unwrap();
        assert_eq!(entry.value.fields["goal"], "explore");
        assert!(!entry.value.fields.contains_key("notes"));
    }

    #[test]
    fn update_managed_state_only_preserves_fields() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("explore"));

        exec(&branch, TransactionInput::new(meta("create"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            )));

        // Update state only, no fields.
        exec(&branch, TransactionInput::new(meta("close"))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("closed".into()),
                Map::new(),
            )));

        let entry = branch.get_latest::<ManagedEntry>(&managed_bound(&branch, "task", "task-1"))
            .unwrap().unwrap();
        assert_eq!(entry.value.state.as_deref(), Some("closed"));
        assert_eq!(entry.value.fields["goal"], "explore");
    }

    #[test]
    fn update_managed_fields_only_preserves_state() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("explore"));

        exec(&branch, TransactionInput::new(meta("create"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            )));

        // Update fields only, state = None → carry forward.
        let mut update_fields = Map::new();
        update_fields.insert("notes".into(), serde_json::json!("found it"));

        exec(&branch, TransactionInput::new(meta("add notes"))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                None,
                update_fields,
            )));

        let entry = branch.get_latest::<ManagedEntry>(&managed_bound(&branch, "task", "task-1"))
            .unwrap().unwrap();
        assert_eq!(entry.value.state.as_deref(), Some("open"));
        assert_eq!(entry.value.fields["notes"], "found it");
        assert_eq!(entry.value.fields["goal"], "explore");
    }

    // --- Mixed operations ---

    #[test]
    fn mixed_operations_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(&branch, TransactionInput::new(meta("setup"))
            .create_entity(Entity::new(
                "server".parse().unwrap(),
                Some(serde_json::json!("Production server")),
                vec![],
                Map::new(),
            ))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                {
                    let mut f = Map::new();
                    f.insert("goal".into(), serde_json::json!("monitor"));
                    f
                },
            )));

        let mut new_fields = Map::new();
        new_fields.insert("goal".into(), serde_json::json!("investigate"));

        let result = exec(&branch, TransactionInput::new(meta("mixed"))
            .create_entity(Entity::new(
                "db-node".parse().unwrap(),
                Some(serde_json::json!("Database node")),
                vec![
                    PropertyValue { property: "status".parse().unwrap(), value: serde_json::json!("healthy"), context: () },
                ],
                entity_meta("initial check"),
            ))
            .update_entity(Entity::new(
                "server".parse().unwrap(),
                None,
                vec![
                    PropertyValue { property: "status".parse().unwrap(), value: serde_json::json!("degraded"), context: () },
                ],
                entity_meta("health check failed"),
            ))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-2".parse().unwrap(),
                Some("open".into()),
                new_fields,
            ))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                {
                    let mut f = Map::new();
                    f.insert("goal".into(), serde_json::json!("escalate"));
                    f
                },
            )));

        let db_node = branch.get_latest::<EntityEntry>(&entity_bound(&branch, "db-node")).unwrap().unwrap();
        assert_eq!(db_node.key.tx_id, result.context.tx_id);

        let task2 = branch.get_latest::<ManagedEntry>(&managed_bound(&branch, "task", "task-2"))
            .unwrap().unwrap();
        assert_eq!(task2.key.tx_id, result.context.tx_id);

        let task1 = branch.get_latest::<ManagedEntry>(&managed_bound(&branch, "task", "task-1"))
            .unwrap().unwrap();
        assert_eq!(task1.value.fields["goal"], "escalate");
    }

    // --- Entity change provenance ---

    #[test]
    fn entity_change_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            )));

        let result = exec(&branch, TransactionInput::new(meta("observe"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![
                    PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                    PropertyValue { property: "city".parse().unwrap(), value: serde_json::json!("London"), context: () },
                ],
                entity_meta("census data"),
            )));

        let alice_slug: crate::io::Slug = "alice".parse().unwrap();
        let age_slug: crate::io::Slug = "age".parse().unwrap();
        let city_slug: crate::io::Slug = "city".parse().unwrap();

        let age_bound = AssertionKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = alice_slug.clone(); k.prop = age_slug.clone(); });
        let city_bound = AssertionKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = alice_slug.clone(); k.prop = city_slug.clone(); });

        let age_entry = branch.storage().get_latest::<AssertionEntry>(&age_bound).unwrap().unwrap();
        let city_entry = branch.storage().get_latest::<AssertionEntry>(&city_bound).unwrap().unwrap();

        assert_eq!(age_entry.value.change_id, city_entry.value.change_id);

        let change = storage_get_exact(&branch, age_entry.value.change_id).unwrap();
        assert_eq!(change.value.entity, alice_slug.as_str());
        assert_eq!(change.value.tx_id, result.context.tx_id);
        assert_eq!(change.value.meta["reasoning"], "census data");
    }

    // --- Touched ---

    #[test]
    fn create_entity_writes_touched() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                entity_meta("first sighting"),
            )));

        let key = TouchedKey {
            branch: branch.id().clone(),
            tx_id: result.context.tx_id,
            entity: "alice".parse().unwrap(),
        };
        let found = branch.storage().get::<TouchedEntry>(&key).unwrap().unwrap();
        assert_eq!(found.value.reasoning, "first sighting");
    }

    #[test]
    fn update_entity_writes_touched() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            )));

        let result = exec(&branch, TransactionInput::new(meta("update"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![
                    PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                ],
                entity_meta("observed age"),
            )));

        let key = TouchedKey {
            branch: branch.id().clone(),
            tx_id: result.context.tx_id,
            entity: "alice".parse().unwrap(),
        };
        let found = branch.storage().get::<TouchedEntry>(&key).unwrap().unwrap();
        assert_eq!(found.value.reasoning, "observed age");
    }

    #[test]
    fn multiple_entities_touched_in_one_tx() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("batch"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Person A")),
                vec![],
                entity_meta("introduce alice"),
            ))
            .create_entity(Entity::new(
                "bob".parse().unwrap(),
                Some(serde_json::json!("Person B")),
                vec![],
                entity_meta("introduce bob"),
            )));

        for (slug, expected) in [("alice", "introduce alice"), ("bob", "introduce bob")] {
            let key = TouchedKey {
                branch: branch.id().clone(),
                tx_id: result.context.tx_id,
                entity: slug.parse().unwrap(),
            };
            let found = branch.storage().get::<TouchedEntry>(&key).unwrap().unwrap();
            assert_eq!(found.value.reasoning, expected);
        }
    }

    #[test]
    fn explicit_touch_overrides_auto() {
        use crate::command::input::transaction::TouchItem;

        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("investigate"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                entity_meta("auto reasoning"),
            ))
            .touch(TouchItem::new("alice".parse().unwrap(), "primary suspect")));

        let key = TouchedKey {
            branch: branch.id().clone(),
            tx_id: result.context.tx_id,
            entity: "alice".parse().unwrap(),
        };
        let found = branch.storage().get::<TouchedEntry>(&key).unwrap().unwrap();
        assert_eq!(found.value.reasoning, "primary suspect");
    }

    #[test]
    fn explicit_touch_without_mutation() {
        use crate::command::input::transaction::TouchItem;

        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("observe"))
            .touch(TouchItem::new("server".parse().unwrap(), "checked health")));

        let key = TouchedKey {
            branch: branch.id().clone(),
            tx_id: result.context.tx_id,
            entity: "server".parse().unwrap(),
        };
        let found = branch.storage().get::<TouchedEntry>(&key).unwrap().unwrap();
        assert_eq!(found.value.reasoning, "checked health");
    }

    // --- Embeddings ---

    mod embedding_tests {
        use super::*;
        use std::sync::Mutex;
        use crate::embed::Embedder;
        use crate::store::entry::embedding::{EmbeddingEntry, EmbeddingKey};
        use crate::io::storage_key::StorageKey;

        /// Deterministic test embedder — returns a fixed vector based on text length.
        struct TestEmbedder {
            calls: Mutex<Vec<String>>,
        }

        impl TestEmbedder {
            fn new() -> Self {
                Self { calls: Mutex::new(Vec::new()) }
            }

            fn call_count(&self) -> usize {
                self.calls.lock().unwrap().len()
            }

            fn last_text(&self) -> String {
                self.calls.lock().unwrap().last().cloned().unwrap_or_default()
            }
        }

        impl Embedder for TestEmbedder {
            fn embed(&self, text: &str) -> Result<Vec<f32>, crate::io::DbError> {
                self.calls.lock().unwrap().push(text.to_string());
                // Deterministic 4-dim vector based on text length.
                let len = text.len() as f32;
                Ok(vec![len, len * 0.5, len * 0.1, 1.0])
            }

            fn dimensions(&self) -> usize {
                4
            }
        }

        fn exec_with_embedder(
            branch: &BranchContext,
            embedder: Arc<dyn Embedder>,
            input: TransactionInput,
        ) -> Transaction<TxMeta> {
            let cmd = cmd();
            let mut state = CommandState::with_embedder(branch.storage(), embedder);
            let result = cmd.execute(branch, &mut state, input).unwrap();
            state.commit().unwrap();
            result
        }

        #[test]
        fn noop_embedder_writes_no_embedding() {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path(), test_config()).unwrap();
            let branch = BranchContext::main(storage);

            exec(&branch, TransactionInput::new(meta("create"))
                .create_entity(Entity::new(
                    "alice".parse().unwrap(),
                    Some(serde_json::json!("A person")),
                    vec![
                        PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                    ],
                    entity_meta("initial"),
                )));

            let bound = EmbeddingKey::bound()
                .with_prefix(|k| {
                    k.branch = branch.id().clone();
                    k.entity = "alice".parse().unwrap();
                });
            let found = branch.storage().get_latest::<EmbeddingEntry>(&bound).unwrap();
            assert!(found.is_none());
        }

        #[test]
        fn create_entity_writes_embedding() {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path(), test_config()).unwrap();
            let branch = BranchContext::main(storage);
            let embedder = Arc::new(TestEmbedder::new());

            let result = exec_with_embedder(&branch, embedder.clone(),
                TransactionInput::new(meta("create"))
                    .create_entity(Entity::new(
                        "alice".parse().unwrap(),
                        Some(serde_json::json!("A person")),
                        vec![
                            PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                        ],
                        entity_meta("initial"),
                    )));

            assert!(embedder.call_count() > 0);

            let bound = EmbeddingKey::bound()
                .with_prefix(|k| {
                    k.branch = branch.id().clone();
                    k.entity = "alice".parse().unwrap();
                });
            let found = branch.storage().get_latest::<EmbeddingEntry>(&bound).unwrap().unwrap();
            assert_eq!(found.key.tx_id, result.context.tx_id);
            assert_eq!(found.value.embedding.len(), 4);
        }

        #[test]
        fn update_entity_writes_new_embedding() {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path(), test_config()).unwrap();
            let branch = BranchContext::main(storage);
            let embedder = Arc::new(TestEmbedder::new());

            exec_with_embedder(&branch, embedder.clone(),
                TransactionInput::new(meta("create"))
                    .create_entity(Entity::new(
                        "alice".parse().unwrap(),
                        Some(serde_json::json!("A person")),
                        vec![],
                        Map::new(),
                    )));

            let result = exec_with_embedder(&branch, embedder.clone(),
                TransactionInput::new(meta("update"))
                    .update_entity(Entity::new(
                        "alice".parse().unwrap(),
                        None,
                        vec![
                            PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                        ],
                        entity_meta("birthday"),
                    )));

            assert!(embedder.call_count() >= 2);

            let bound = EmbeddingKey::bound()
                .with_prefix(|k| {
                    k.branch = branch.id().clone();
                    k.entity = "alice".parse().unwrap();
                });
            let found = branch.storage().get_latest::<EmbeddingEntry>(&bound).unwrap().unwrap();
            assert_eq!(found.key.tx_id, result.context.tx_id);
        }

        #[test]
        fn embed_text_includes_description_and_properties() {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path(), test_config()).unwrap();
            let branch = BranchContext::main(storage);
            let embedder = Arc::new(TestEmbedder::new());

            exec_with_embedder(&branch, embedder.clone(),
                TransactionInput::new(meta("create"))
                    .create_entity(Entity::new(
                        "server".parse().unwrap(),
                        Some(serde_json::json!("Production server")),
                        vec![
                            PropertyValue { property: "status".parse().unwrap(), value: serde_json::json!("healthy"), context: () },
                            PropertyValue { property: "zone".parse().unwrap(), value: serde_json::json!("us-east"), context: () },
                        ],
                        entity_meta("initial"),
                    )));

            let text = embedder.last_text();
            assert!(text.contains("server"));
            assert!(text.contains("Production server"));
            assert!(text.contains("status"));
            assert!(text.contains("zone"));
        }

        #[test]
        fn similar_entities_returns_results() {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path(), test_config()).unwrap();
            let branch = BranchContext::main(storage);
            let embedder = Arc::new(TestEmbedder::new());

            exec_with_embedder(&branch, embedder.clone(),
                TransactionInput::new(meta("create"))
                    .create_entity(Entity::new(
                        "alice".parse().unwrap(),
                        Some(serde_json::json!("A person")),
                        vec![
                            PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                        ],
                        entity_meta("initial"),
                    ))
                    .create_entity(Entity::new(
                        "bob".parse().unwrap(),
                        Some(serde_json::json!("Another person")),
                        vec![
                            PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                        ],
                        entity_meta("initial"),
                    )));

            // Query with a vector similar to what TestEmbedder produces.
            let query = vec![50.0, 25.0, 5.0, 1.0];
            let results = similarity::similar_entities(&branch, &query, 10, 0.0, None).unwrap();

            assert_eq!(results.len(), 2);
            // Both should have high similarity (vectors are in the same direction).
            for (_, score) in &results {
                assert!(*score > 0.9);
            }
        }

        #[test]
        fn similar_entities_respects_limit() {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path(), test_config()).unwrap();
            let branch = BranchContext::main(storage);
            let embedder = Arc::new(TestEmbedder::new());

            for name in ["alice", "bob", "charlie"] {
                exec_with_embedder(&branch, embedder.clone(),
                    TransactionInput::new(meta("create"))
                        .create_entity(Entity::new(
                            name.parse().unwrap(),
                            Some(serde_json::json!("person")),
                            vec![],
                            Map::new(),
                        )));
            }

            let query = vec![10.0, 5.0, 1.0, 1.0];
            let results = similarity::similar_entities(&branch, &query, 2, 0.0, None).unwrap();
            assert_eq!(results.len(), 2);
        }

        #[test]
        fn similar_entities_filters_by_min_similarity() {
            let dir = tempfile::tempdir().unwrap();
            let storage = Storage::open(dir.path(), test_config()).unwrap();
            let branch = BranchContext::main(storage);
            let embedder = Arc::new(TestEmbedder::new());

            exec_with_embedder(&branch, embedder.clone(),
                TransactionInput::new(meta("create"))
                    .create_entity(Entity::new(
                        "alice".parse().unwrap(),
                        Some(serde_json::json!("A person")),
                        vec![],
                        Map::new(),
                    )));

            // Orthogonal query — should have low similarity.
            let query = vec![0.0, 0.0, 0.0, 0.0];
            let results = similarity::similar_entities(&branch, &query, 10, 0.5, None).unwrap();
            assert!(results.is_empty());
        }
    }

    // --- Transaction log ---

    #[test]
    fn transaction_log_records_all_operations() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage.clone());

        // Step 1: create two entities.
        let tx1 = exec(&branch, TransactionInput::new(meta("setup"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Person A")),
                vec![PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () }],
                entity_meta("census"),
            ))
            .create_entity(Entity::new(
                "bob".parse().unwrap(),
                Some(serde_json::json!("Person B")),
                vec![],
                Map::new(),
            )));

        // Verify tx1 log.
        let log1 = storage.get::<TransactionLogEntry>(
            &TransactionLogKey { tx_id: tx1.context.tx_id }
        ).unwrap().unwrap();
        assert_eq!(log1.value.branch, "main");
        assert_eq!(log1.value.created.len(), 2);
        assert_eq!(log1.value.created[0].entity, "alice");
        assert_eq!(log1.value.created[0].properties.len(), 1);
        assert_eq!(log1.value.created[0].properties[0], "age");
        assert_eq!(log1.value.created[1].entity, "bob");
        assert!(log1.value.created[1].properties.is_empty());
        assert!(log1.value.updated.is_empty());
        assert!(log1.value.touched.is_empty());
        assert!(log1.value.deleted.is_empty());

        // Step 2: update + touch + delete in one transaction.
        let tx2 = exec(&branch, TransactionInput::new(meta("mixed ops"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(31), context: () }],
                entity_meta("birthday"),
            ))
            .delete_entity(DeleteItem::new("bob".parse().unwrap(), serde_json::json!("no longer relevant")))
            .touch(TouchItem::new("alice".parse().unwrap(), "still relevant")));

        let log2 = storage.get::<TransactionLogEntry>(
            &TransactionLogKey { tx_id: tx2.context.tx_id }
        ).unwrap().unwrap();
        assert_eq!(log2.value.branch, "main");
        assert!(log2.value.created.is_empty());
        assert_eq!(log2.value.updated.len(), 1);
        assert_eq!(log2.value.updated[0].entity, "alice");
        assert_eq!(log2.value.updated[0].properties[0], "age");
        assert_eq!(log2.value.deleted[0].entity, "bob");
        assert!(log2.value.deleted[0].properties.is_empty());
        assert_eq!(log2.value.touched[0], "alice");
    }

    // --- Duplicate slug detection ---

    #[test]
    fn duplicate_entity_slug_in_transaction_fails() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let c = cmd();
        let mut state = CommandState::new(branch.storage());
        let err = c.execute(&branch, &mut state, TransactionInput::new(meta("dup"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("First")),
                vec![],
                Map::new(),
            ))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Second")),
                vec![],
                Map::new(),
            ))
        ).unwrap_err();
        assert!(err.to_string().contains("duplicate entity in transaction: alice"));
    }

    #[test]
    fn duplicate_managed_slug_in_transaction_fails() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("task A"));

        let c = cmd();
        let mut state = CommandState::new(branch.storage());
        let err = c.execute(&branch, &mut state, TransactionInput::new(meta("dup"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields.clone(),
            ))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            ))
        ).unwrap_err();
        assert!(err.to_string().contains("duplicate managed item in transaction: task/task-1"));
    }

    #[test]
    fn different_entity_slugs_in_transaction_ok() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let result = exec(&branch, TransactionInput::new(meta("two entities"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Person A")),
                vec![],
                Map::new(),
            ))
            .create_entity(Entity::new(
                "bob".parse().unwrap(),
                Some(serde_json::json!("Person B")),
                vec![],
                Map::new(),
            )));

        assert_eq!(result.meta["reasoning"], "two entities");
    }
}
