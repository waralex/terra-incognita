//! ListTransactions — lists recent transactions on a branch.

use uuid::Uuid;

use crate::command::Command;
use crate::command::CommandState;
use crate::command::input::list_transactions::ListTransactionsQuery;
use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::TxMeta;
use crate::io::DbError;
use crate::io::storage_key::StorageKey;
use crate::store::branch_context::BranchContext;
use crate::store::entry::transaction::{TransactionEntry, TransactionKey};

/// Lists recent transactions (most recent first), walking ancestry if needed.
pub struct ListTransactions;

impl Command for ListTransactions {
    type Input = ListTransactionsQuery;
    type Output = Vec<Transaction<TxMeta>>;

    fn execute(
        &self,
        branch: &BranchContext,
        _state: &mut CommandState,
        input: Self::Input,
    ) -> Result<Self::Output, DbError> {
        let at_tx = match input.at_tx {
            Some(tx) => tx,
            None => branch.head_tx()?.unwrap_or(Uuid::nil()),
        };

        let mut result = Vec::new();

        let bound = TransactionKey::bound()
            .with_prefix(|k| k.branch = branch.id().clone())
            .with_upper(|k| k.tx_id = at_tx);

        let iter = branch.storage().scan_rev::<TransactionEntry>(&bound)?;
        for entry_result in iter {
            if result.len() >= input.limit {
                break;
            }
            let entry = entry_result?;
            result.push(Transaction {
                meta: entry.value.meta,
                context: TxMeta {
                    tx_id: entry.key.tx_id,
                    branch: entry.key.branch,
                    reasoning: None,
                },
            });
        }

        // Walk ancestry for more transactions if needed.
        for ancestor in branch.ancestry() {
            if result.len() >= input.limit {
                break;
            }
            let ancestor_bound = TransactionKey::bound()
                .with_prefix(|k| k.branch = ancestor.branch.clone())
                .with_upper(|k| k.tx_id = ancestor.branch_point_tx);

            let iter = branch.storage().scan_rev::<TransactionEntry>(&ancestor_bound)?;
            for entry_result in iter {
                if result.len() >= input.limit {
                    break;
                }
                let entry = entry_result?;
                result.push(Transaction {
                    meta: entry.value.meta,
                    context: TxMeta {
                        tx_id: entry.key.tx_id,
                        branch: entry.key.branch,
                        reasoning: None,
                    },
                });
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use serde_json::{Map, Value};
    use indoc::indoc;

    use super::*;
    use crate::command::executor::transaction::ExecuteTransaction;
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

    fn exec_tx(branch: &BranchContext, input: TransactionInput) {
        let cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
    }

    #[test]
    fn transaction_limit_respected() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        for i in 0..5 {
            exec_tx(&branch, TransactionInput::new(meta(&format!("tx-{}", i))));
        }

        let cmd = ListTransactions;
        let mut state = CommandState::new(branch.storage());
        let txs = cmd.execute(&branch, &mut state, ListTransactionsQuery::new(None, 3)).unwrap();
        assert_eq!(txs.len(), 3);
        assert_eq!(txs[0].meta["reasoning"], "tx-4");
        assert_eq!(txs[2].meta["reasoning"], "tx-2");
    }
}
