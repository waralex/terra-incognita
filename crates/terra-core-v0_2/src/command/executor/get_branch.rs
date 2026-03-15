//! GetBranch — reads branch metadata.

use uuid::Uuid;

use crate::command::Command;
use crate::command::CommandState;
use crate::command::input::get_branch::GetBranchQuery;
use crate::domain::branch::Branch;
use crate::domain::tx_meta::TxMeta;
use crate::io::DbError;
use crate::store::branch_context::{BranchContext, main_branch_slug};
use crate::store::entry::branch::{BranchEntry, BranchKey};

/// Reads branch metadata.
pub struct GetBranch;

impl Command for GetBranch {
    type Input = GetBranchQuery;
    type Output = Branch<TxMeta>;

    fn execute(
        &self,
        branch: &BranchContext,
        _state: &mut CommandState,
        _input: Self::Input,
    ) -> Result<Self::Output, DbError> {
        let slug = branch.id().clone();

        if slug == main_branch_slug() {
            return Ok(Branch {
                slug: slug.clone(),
                parent: slug,
                meta: serde_json::Map::new(),
                context: TxMeta {
                    tx_id: Uuid::nil(),
                    branch: main_branch_slug(),
                    reasoning: None,
                },
            });
        }

        let key = BranchKey { branch: slug.clone() };
        let entry = branch.storage().get::<BranchEntry>(&key)?
            .ok_or_else(|| DbError::Storage(format!("branch not found: {}", slug)))?;

        let parent = entry.value.parent_branch_slug.parse()
            .map_err(|e: crate::io::slug::SlugError| DbError::Storage(e.to_string()))?;

        Ok(Branch {
            slug,
            parent,
            meta: entry.value.meta,
            context: TxMeta {
                tx_id: entry.value.created_from_tx,
                branch: main_branch_slug(),
                reasoning: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use serde_json::{Map, Value};
    use indoc::indoc;

    use super::*;
    use crate::command::executor::checkout::ExecuteCheckout;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::checkout::CheckoutInput;
    use crate::command::input::transaction::TransactionInput;
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::validator::DomainValidator;
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
            managed_types:
              task:
                fields:
                  goal: { type: json, required: true }
                lifecycle:
                  initial: open
                  visible: [open]
        "}).unwrap())
    }

    fn meta(r: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(r.into()));
        m
    }

    #[test]
    fn main_branch_returns_hardcoded() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let cmd = GetBranch;
        let mut state = CommandState::new(branch.storage());
        let result = cmd.execute(&branch, &mut state, GetBranchQuery::new()).unwrap();
        assert_eq!(result.slug.as_str(), "main");
        assert_eq!(result.parent.as_str(), "main");
        assert_eq!(result.context.tx_id, Uuid::nil());
    }

    #[test]
    fn child_branch_returns_record() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        // Create a transaction so checkout has a tx to branch from.
        let tx_cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut cs = CommandState::new(&storage);
        tx_cmd.execute(&main, &mut cs, TransactionInput::new(meta("seed"))).unwrap();
        cs.commit().unwrap();

        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut cs = CommandState::new(&storage);
        checkout_cmd.execute(&main, &mut cs, CheckoutInput::new(
            "child".parse().unwrap(),
            meta("explore"),
            None,
            TransactionInput::new(meta("init child")),
        )).unwrap();
        cs.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let cmd = GetBranch;
        let mut state = CommandState::new(child.storage());
        let result = cmd.execute(&child, &mut state, GetBranchQuery::new()).unwrap();
        assert_eq!(result.slug.as_str(), "child");
        assert_eq!(result.parent.as_str(), "main");
    }
}
