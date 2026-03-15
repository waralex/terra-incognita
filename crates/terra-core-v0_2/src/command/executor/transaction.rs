//! ExecuteTransaction — commits a validated transaction to a branch.

use uuid::Uuid;

use crate::command::Command;
use crate::command::input::transaction::TransactionInput;
use crate::domain::entity::Entity;
use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::TxMeta;
use crate::io::DbError;
use crate::io::WriteBatch;
use crate::store::branch_context::BranchContext;
use crate::store::entry::entity::{EntityEntry, EntityKey, EntityValue};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey, TransactionValue};

/// Executes a validated domain transaction against a branch.
pub struct ExecuteTransaction;

impl ExecuteTransaction {
    fn create_entity(
        &self,
        branch: &BranchContext,
        batch: &mut WriteBatch,
        tx_id: Uuid,
        entity: &Entity,
    ) -> Result<(), DbError> {
        if branch.entity_exists(&entity.slug)? {
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

        Ok(())
    }
}

impl Command for ExecuteTransaction {
    type Input = TransactionInput;
    type Output = Transaction<TxMeta>;

    fn execute(&self, branch: &BranchContext, input: Self::Input) -> Result<Self::Output, DbError> {
        let tx_id = Uuid::now_v7();
        let mut batch = branch.storage().db.batch();

        for entity in &input.create_entities {
            self.create_entity(branch, &mut batch, tx_id, entity)?;
        }

        // TODO: update_entities, create_managed, update_managed

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
    use crate::store::storage::Storage;
    use serde_json::{Map, Value};

    fn test_config() -> Arc<ProjectConfig> {
        Arc::new(ProjectConfig::builder()
            .data_dir("./data".into())
            .schema_path("./schema.yaml".into())
            .build())
    }

    fn meta(reasoning: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(reasoning.into()));
        m
    }

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
            ));

        let cmd = ExecuteTransaction;
        let result = cmd.execute(&branch, input).unwrap();

        assert_eq!(result.meta["reasoning"], "introduce alice");
        assert_eq!(result.context.branch.as_str(), "main");

        // Verify entity record was written
        let entry = branch.get_latest_entity(&"alice".parse().unwrap()).unwrap().unwrap();
        assert_eq!(entry.key.entity.as_str(), "alice");
        assert_eq!(entry.value.description, Some(serde_json::json!("A person")));
    }

    #[test]
    fn duplicate_entity_slug_fails() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let cmd = ExecuteTransaction;

        let input1 = TransactionInput::new(meta("first"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("First")),
                vec![],
            ));
        cmd.execute(&branch, input1).unwrap();

        let input2 = TransactionInput::new(meta("second"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("Duplicate")),
                vec![],
            ));
        let err = cmd.execute(&branch, input2).unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn empty_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let cmd = ExecuteTransaction;
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
            ))
            .create_entity(Entity::new(
                "bob".parse().unwrap(),
                Some(serde_json::json!("Person B")),
                vec![],
            ));

        let cmd = ExecuteTransaction;
        let result = cmd.execute(&branch, input).unwrap();

        for name in ["alice", "bob"] {
            let entry = branch.get_latest_entity(&name.parse().unwrap()).unwrap().unwrap();
            assert_eq!(entry.key.entity.as_str(), name);
            assert_eq!(entry.key.tx_id, result.context.tx_id);
        }
    }

    #[test]
    fn entity_exists_after_create() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        assert!(!branch.entity_exists(&"alice".parse().unwrap()).unwrap());

        let cmd = ExecuteTransaction;
        let input = TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
            ));
        cmd.execute(&branch, input).unwrap();

        assert!(branch.entity_exists(&"alice".parse().unwrap()).unwrap());
    }
}
