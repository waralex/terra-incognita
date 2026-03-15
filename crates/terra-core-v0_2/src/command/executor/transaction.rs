//! ExecuteTransaction — validates and commits a transaction to a branch.

use uuid::Uuid;

use crate::command::Command;
use crate::command::input::transaction::TransactionInput;
use crate::domain::entity::Entity;
use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::TxMeta;
use crate::domain::validator::DomainValidator;
use crate::io::DbError;
use crate::io::WriteBatch;
use crate::io::storage_key::StorageKey;
use crate::store::branch_context::BranchContext;
use crate::store::entry::assertion::{AssertionEntry, AssertionKey, AssertionValue};
use crate::store::entry::entity::{EntityEntry, EntityKey, EntityValue};
use crate::store::entry::entity_change::{EntityChangeEntry, EntityChangeKey, EntityChangeValue};
use crate::store::entry::managed::{ManagedEntry, ManagedKey, ManagedValue};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey, TransactionValue};

/// Validates and commits a transaction to a branch.
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
    fn write_assertions(
        &self,
        branch: &BranchContext,
        batch: &mut WriteBatch,
        tx_id: Uuid,
        entity: &Entity,
    ) -> Result<(), DbError> {
        if entity.properties.is_empty() {
            return Ok(());
        }

        let change_id = Uuid::now_v7();
        let entity_id = entity.slug.hash();

        batch.put(&EntityChangeEntry {
            key: EntityChangeKey { change_id },
            value: EntityChangeValue {
                entity_id,
                tx_id,
                meta: entity.meta.clone(),
            },
        })?;

        for pv in &entity.properties {
            batch.put(&AssertionEntry {
                key: AssertionKey {
                    branch: branch.id().clone(),
                    prop_id: pv.property.hash(),
                    tx_id,
                    change_id,
                    entity_id,
                },
                value: AssertionValue {
                    value: pv.value.clone(),
                },
            })?;
        }

        Ok(())
    }

    fn create_entity(
        &self,
        branch: &BranchContext,
        batch: &mut WriteBatch,
        tx_id: Uuid,
        entity: &Entity,
    ) -> Result<(), DbError> {
        let bound = EntityKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = entity.slug.clone(); });
        if branch.exists::<EntityEntry>(&bound)? {
            return Err(DbError::Storage(format!(
                "entity already exists: {}", entity.slug
            )));
        }

        batch.put(&EntityEntry {
            key: EntityKey {
                branch: branch.id().clone(),
                entity: entity.slug.clone(),
                tx_id,
            },
            value: EntityValue {
                description: entity.description.clone(),
            },
        })?;

        self.write_assertions(branch, batch, tx_id, entity)?;

        Ok(())
    }

    fn update_entity(
        &self,
        branch: &BranchContext,
        batch: &mut WriteBatch,
        tx_id: Uuid,
        entity: &Entity,
    ) -> Result<(), DbError> {
        let bound = EntityKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = entity.slug.clone(); });
        if !branch.exists::<EntityEntry>(&bound)? {
            return Err(DbError::Storage(format!(
                "entity not found: {}", entity.slug
            )));
        }

        if entity.description.is_some() {
            batch.put(&EntityEntry {
                key: EntityKey {
                    branch: branch.id().clone(),
                    entity: entity.slug.clone(),
                    tx_id,
                },
                value: EntityValue {
                    description: entity.description.clone(),
                },
            })?;
        }

        self.write_assertions(branch, batch, tx_id, entity)?;

        Ok(())
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
        if !branch.exists::<ManagedEntry>(&bound)? {
            return Err(DbError::Storage(format!(
                "managed item not found: {}/{}", managed.type_name, managed.slug
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
}

impl Command for ExecuteTransaction {
    type Input = TransactionInput;
    type Output = Transaction<TxMeta>;

    fn execute(&self, branch: &BranchContext, input: Self::Input) -> Result<Self::Output, DbError> {
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
        let mut batch = branch.storage().db.batch();

        for entity in &input.create_entities {
            self.create_entity(branch, &mut batch, tx_id, entity)?;
        }

        for entity in &input.update_entities {
            self.update_entity(branch, &mut batch, tx_id, entity)?;
        }

        for managed in &input.create_managed {
            self.create_managed_item(branch, &mut batch, tx_id, managed)?;
        }

        for managed in &input.update_managed {
            self.update_managed_item(branch, &mut batch, tx_id, managed)?;
        }

        batch.put(&TransactionEntry {
            key: TransactionKey {
                branch: branch.id().clone(),
                tx_id,
            },
            value: TransactionValue { meta: input.meta.clone() },
        })?;

        batch.commit()?;

        Ok(Transaction {
            meta: input.meta,
            context: TxMeta {
                tx_id,
                branch: branch.id().clone(),
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
    use crate::store::storage::Storage;
    use serde_json::{Map, Value};

    use indoc::indoc;
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

    // --- Create entity ---

    #[test]
    fn create_single_entity() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage.clone());

        let input = TransactionInput::new(meta("introduce alice"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            ));

        let cmd = cmd();
        let result = cmd.execute(&branch, input).unwrap();

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

        let cmd = cmd();

        let input1 = TransactionInput::new(meta("first"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("First")),
                vec![],
                Map::new(),
            ));
        cmd.execute(&branch, input1).unwrap();

        let input2 = TransactionInput::new(meta("second"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Duplicate")),
                vec![],
                Map::new(),
            ));
        let err = cmd.execute(&branch, input2).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn empty_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let cmd = cmd();
        let input = TransactionInput::new(meta("no-op"));
        let result = cmd.execute(&branch, input).unwrap();

        assert_eq!(result.meta["reasoning"], "no-op");
    }

    #[test]
    fn multiple_entities_in_one_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let input = TransactionInput::new(meta("batch"))
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
            ));

        let cmd = cmd();
        let result = cmd.execute(&branch, input).unwrap();

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

        let cmd = cmd();
        let input = TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            ));
        cmd.execute(&branch, input).unwrap();

        assert!(branch.exists::<EntityEntry>(&prefix).unwrap());
    }

    #[test]
    fn create_entity_with_initial_properties() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let cmd = cmd();
        let result = cmd.execute(&branch, TransactionInput::new(meta("create with props"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                    PropertyValue { property: "city".parse().unwrap(), value: serde_json::json!("London"), context: () },
                ],
                entity_meta("initial observation"),
            ))
        ).unwrap();

        // Verify entity record
        let entry = branch.get_latest::<EntityEntry>(&entity_bound(&branch, "alice")).unwrap().unwrap();
        assert_eq!(entry.value.description, Some(serde_json::json!("A person")));

        // Verify assertions via direct DB scan
        let age_slug: crate::io::Slug = "age".parse().unwrap();
        let alice_slug: crate::io::Slug = "alice".parse().unwrap();
        let bound = AssertionKey::bound()
            .with_prefix(|k| {
                k.branch = branch.id().clone();
                k.prop_id = age_slug.hash();
            });
        let found = branch.storage().get_latest::<AssertionEntry>(&bound).unwrap().unwrap();
        assert_eq!(found.value.value, serde_json::json!(30));
        assert_eq!(found.key.entity_id, alice_slug.hash());
        assert_eq!(found.key.tx_id, result.context.tx_id);

        // Verify entity change entry
        let change = storage_get_exact(&branch, found.key.change_id).unwrap();
        assert_eq!(change.value.meta["reasoning"], "initial observation");
        assert_eq!(change.value.entity_id, alice_slug.hash());
    }

    /// Helper: get EntityChangeEntry by exact change_id.
    fn storage_get_exact(branch: &BranchContext, change_id: Uuid) -> Result<EntityChangeEntry, DbError> {
        let key = EntityChangeKey { change_id };
        branch.storage().db.get::<EntityChangeEntry>(&key)?
            .ok_or_else(|| DbError::Storage("entity change not found".into()))
    }

    // --- Update entity ---

    #[test]
    fn update_entity_writes_assertions() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let cmd = cmd();

        // Create entity first
        cmd.execute(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            ))
        ).unwrap();

        // Update with properties
        let result = cmd.execute(&branch, TransactionInput::new(meta("update"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![
                    PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                ],
                entity_meta("age observed"),
            ))
        ).unwrap();

        // Verify assertion written
        let age_slug: crate::io::Slug = "age".parse().unwrap();
        let bound = AssertionKey::bound()
            .with_prefix(|k| {
                k.branch = branch.id().clone();
                k.prop_id = age_slug.hash();
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

        let err = cmd.execute(&branch, TransactionInput::new(meta("update missing"))
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
        let cmd = cmd();

        cmd.execute(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Original")),
                vec![],
                Map::new(),
            ))
        ).unwrap();

        cmd.execute(&branch, TransactionInput::new(meta("update desc"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Updated description")),
                vec![],
                Map::new(),
            ))
        ).unwrap();

        let entry = branch.get_latest::<EntityEntry>(&entity_bound(&branch, "alice")).unwrap().unwrap();
        assert_eq!(entry.value.description, Some(serde_json::json!("Updated description")));
    }

    // --- Create managed ---

    #[test]
    fn create_managed_item() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let cmd = cmd();

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("explore"));

        cmd.execute(&branch, TransactionInput::new(meta("create task"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            ))
        ).unwrap();

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
        let cmd = cmd();

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("first"));

        cmd.execute(&branch, TransactionInput::new(meta("create"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields.clone(),
            ))
        ).unwrap();

        let err = cmd.execute(&branch, TransactionInput::new(meta("duplicate"))
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
        let cmd = cmd();

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("explore"));

        cmd.execute(&branch, TransactionInput::new(meta("create"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            ))
        ).unwrap();

        let mut updated_fields = Map::new();
        updated_fields.insert("goal".into(), serde_json::json!("explore deeply"));
        updated_fields.insert("notes".into(), serde_json::json!("found something"));

        cmd.execute(&branch, TransactionInput::new(meta("update"))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                updated_fields,
            ))
        ).unwrap();

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

        let err = cmd.execute(&branch, TransactionInput::new(meta("update missing"))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "ghost".parse().unwrap(),
                Some("open".into()),
                Map::new(),
            ))
        ).unwrap_err();
        assert!(err.to_string().contains("managed item not found: task/ghost"));
    }

    // --- Mixed operations ---

    #[test]
    fn mixed_operations_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let cmd = cmd();

        // Pre-create entity and managed item
        cmd.execute(&branch, TransactionInput::new(meta("setup"))
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
            ))
        ).unwrap();

        // Mixed: create new entity + update existing entity + create managed + update managed
        let mut new_fields = Map::new();
        new_fields.insert("goal".into(), serde_json::json!("investigate"));

        let result = cmd.execute(&branch, TransactionInput::new(meta("mixed"))
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
            ))
        ).unwrap();

        // Verify all operations happened with same tx_id
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
        let cmd = cmd();

        cmd.execute(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            ))
        ).unwrap();

        let result = cmd.execute(&branch, TransactionInput::new(meta("observe"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![
                    PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                    PropertyValue { property: "city".parse().unwrap(), value: serde_json::json!("London"), context: () },
                ],
                entity_meta("census data"),
            ))
        ).unwrap();

        // Both assertions share the same change_id
        let age_slug: crate::io::Slug = "age".parse().unwrap();
        let city_slug: crate::io::Slug = "city".parse().unwrap();

        let age_bound = AssertionKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.prop_id = age_slug.hash(); });
        let city_bound = AssertionKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.prop_id = city_slug.hash(); });

        let age_entry = branch.storage().get_latest::<AssertionEntry>(&age_bound).unwrap().unwrap();
        let city_entry = branch.storage().get_latest::<AssertionEntry>(&city_bound).unwrap().unwrap();

        assert_eq!(age_entry.key.change_id, city_entry.key.change_id);

        // Change entry links back to entity and tx
        let change = storage_get_exact(&branch, age_entry.key.change_id).unwrap();
        let alice_slug: crate::io::Slug = "alice".parse().unwrap();
        assert_eq!(change.value.entity_id, alice_slug.hash());
        assert_eq!(change.value.tx_id, result.context.tx_id);
        assert_eq!(change.value.meta["reasoning"], "census data");
    }
}
