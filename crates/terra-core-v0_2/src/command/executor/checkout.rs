//! ExecuteCheckout — creates a branch from the current BranchContext
//! and runs a first transaction on the new branch.

use uuid::Uuid;

use crate::command::Command;
use crate::command::executor::transaction::ExecuteTransaction;
use crate::command::input::checkout::CheckoutInput;
use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::TxMeta;
use crate::domain::validator::DomainValidator;
use crate::io::DbError;
use crate::io::slug::Slug;
use crate::store::branch_context::BranchContext;
use crate::store::entry::branch::{BranchEntry, BranchKey, BranchValue};

/// Result of a checkout operation.
#[derive(Debug)]
pub struct CheckoutOutput {
    /// Slug of the newly created branch.
    pub branch: Slug,
    /// Transaction on the parent branch that serves as the branch point.
    pub created_from_tx: Uuid,
    /// Result of the first transaction on the new branch.
    pub transaction: Transaction<TxMeta>,
}

/// Creates a branch and executes a first transaction on it.
pub struct ExecuteCheckout {
    validator: DomainValidator,
}

impl ExecuteCheckout {
    /// Create an executor with the given validator.
    pub fn new(validator: DomainValidator) -> Self {
        Self { validator }
    }
}

impl Command for ExecuteCheckout {
    type Input = CheckoutInput;
    type Output = CheckoutOutput;

    fn execute(&self, parent: &BranchContext, input: Self::Input) -> Result<Self::Output, DbError> {
        // 1. Validate branch meta.
        self.validator.check_branch(&input.meta)?;

        // 2. Resolve branch point.
        let created_from_tx = match input.created_from_tx {
            Some(tx_id) => tx_id,
            None => parent.head_tx()?
                .ok_or_else(|| DbError::Storage(
                    format!("no transactions on branch: {}", parent.id())
                ))?,
        };

        // 3. Check slug uniqueness.
        let key = BranchKey { branch: input.slug.clone() };
        if parent.storage().get::<BranchEntry>(&key)?.is_some() {
            return Err(DbError::Storage(
                format!("branch already exists: {}", input.slug)
            ));
        }

        // 4. Write BranchEntry.
        let entry = BranchEntry {
            key: BranchKey { branch: input.slug.clone() },
            value: BranchValue {
                slug: input.slug.to_string(),
                meta: input.meta,
                parent_branch_slug: parent.id().to_string(),
                created_from_tx,
            },
        };
        let mut batch = parent.storage().batch();
        batch.put(&entry)?;
        batch.commit()?;

        // 5. Open new BranchContext.
        let new_branch = BranchContext::open(parent.storage().clone(), input.slug.clone())?;

        // 6. Execute first transaction.
        let cmd = ExecuteTransaction::new(self.validator.clone());
        let tx_result = cmd.execute(&new_branch, input.transaction)?;

        Ok(CheckoutOutput {
            branch: input.slug,
            created_from_tx,
            transaction: tx_result,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use serde_json::{Map, Value};
    use indoc::indoc;

    use super::*;
    use crate::command::input::transaction::TransactionInput;
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::Entity;
    use crate::io::storage_key::StorageKey;
    use crate::store::entry::entity::{EntityEntry, EntityKey};
    use crate::store::storage::Storage;

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
            branch_meta:
              reasoning:
                type: text
                required: true
        "}).unwrap())
    }

    fn validator() -> DomainValidator {
        DomainValidator::new(test_schema())
    }

    fn tx_meta(reasoning: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(reasoning.into()));
        m
    }

    fn commit_tx(branch: &BranchContext, reasoning: &str) -> Transaction<TxMeta> {
        let cmd = ExecuteTransaction::new(validator());
        cmd.execute(branch, TransactionInput::new(tx_meta(reasoning))).unwrap()
    }

    fn entity_bound(branch: &BranchContext, slug: &str) -> crate::io::KeyBound<EntityKey> {
        EntityKey::bound()
            .with_prefix(|k| { k.branch = branch.id().clone(); k.entity = slug.parse().unwrap(); })
    }

    // --- Tests ---

    #[test]
    fn checkout_creates_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        commit_tx(&main, "seed");

        let cmd = ExecuteCheckout::new(validator());
        let result = cmd.execute(&main, CheckoutInput::new(
            "feature".parse().unwrap(),
            tx_meta("explore feature"),
            None,
            TransactionInput::new(tx_meta("first on branch")),
        )).unwrap();

        assert_eq!(result.branch.as_str(), "feature");
        assert_eq!(result.transaction.context.branch.as_str(), "feature");

        // BranchEntry was written.
        let key = BranchKey { branch: "feature".parse().unwrap() };
        let entry = storage.get::<BranchEntry>(&key).unwrap().unwrap();
        assert_eq!(entry.value.slug, "feature");
        assert_eq!(entry.value.parent_branch_slug, "main");

        // BranchContext opens successfully.
        let child = storage.branch("feature".parse().unwrap()).unwrap();
        assert_eq!(child.id().as_str(), "feature");
        assert_eq!(child.ancestry().len(), 1);
    }

    #[test]
    fn checkout_with_entity() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        commit_tx(&main, "seed");

        let cmd = ExecuteCheckout::new(validator());
        let result = cmd.execute(&main, CheckoutInput::new(
            "feature".parse().unwrap(),
            tx_meta("explore"),
            None,
            TransactionInput::new(tx_meta("create entity on branch"))
                .create_entity(Entity::new(
                    "alice".parse().unwrap(),
                    Some(serde_json::json!("A person")),
                    vec![],
                    Map::new(),
                )),
        )).unwrap();

        assert_eq!(result.transaction.context.branch.as_str(), "feature");

        // Entity exists on the new branch.
        let child = storage.branch("feature".parse().unwrap()).unwrap();
        let entry = child.get_latest::<EntityEntry>(&entity_bound(&child, "alice")).unwrap();
        assert!(entry.is_some());

        // Entity does NOT exist on main.
        let entry = main.get_latest::<EntityEntry>(&entity_bound(&main, "alice")).unwrap();
        assert!(entry.is_none());
    }

    #[test]
    fn checkout_duplicate_slug_fails() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        commit_tx(&main, "seed");

        let cmd = ExecuteCheckout::new(validator());
        cmd.execute(&main, CheckoutInput::new(
            "feature".parse().unwrap(),
            tx_meta("first"),
            None,
            TransactionInput::new(tx_meta("init")),
        )).unwrap();

        let err = cmd.execute(&main, CheckoutInput::new(
            "feature".parse().unwrap(),
            tx_meta("second"),
            None,
            TransactionInput::new(tx_meta("init")),
        )).unwrap_err();
        assert!(err.to_string().contains("branch already exists: feature"));
    }

    #[test]
    fn checkout_from_specific_tx() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        let tx1 = commit_tx(&main, "first");
        let _tx2 = commit_tx(&main, "second");

        let cmd = ExecuteCheckout::new(validator());
        let result = cmd.execute(&main, CheckoutInput::new(
            "from-first".parse().unwrap(),
            tx_meta("branch from first tx"),
            Some(tx1.context.tx_id),
            TransactionInput::new(tx_meta("init")),
        )).unwrap();

        assert_eq!(result.created_from_tx, tx1.context.tx_id);
    }

    #[test]
    fn checkout_from_head() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        let _tx1 = commit_tx(&main, "first");
        let tx2 = commit_tx(&main, "second");

        let cmd = ExecuteCheckout::new(validator());
        let result = cmd.execute(&main, CheckoutInput::new(
            "from-head".parse().unwrap(),
            tx_meta("branch from head"),
            None,
            TransactionInput::new(tx_meta("init")),
        )).unwrap();

        assert_eq!(result.created_from_tx, tx2.context.tx_id);
    }

    #[test]
    fn checkout_validates_meta() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        commit_tx(&main, "seed");

        let cmd = ExecuteCheckout::new(validator());
        // Missing required "reasoning" field in branch meta.
        let err = cmd.execute(&main, CheckoutInput::new(
            "bad".parse().unwrap(),
            Map::new(),
            None,
            TransactionInput::new(tx_meta("init")),
        )).unwrap_err();
        assert!(err.to_string().contains("missing required field"));
    }

    #[test]
    fn checkout_empty_parent_fails() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        // No transactions committed.

        let cmd = ExecuteCheckout::new(validator());
        let err = cmd.execute(&main, CheckoutInput::new(
            "orphan".parse().unwrap(),
            tx_meta("no parent tx"),
            None,
            TransactionInput::new(tx_meta("init")),
        )).unwrap_err();
        assert!(err.to_string().contains("no transactions on branch: main"));
    }
}
