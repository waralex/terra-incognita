//! GetTransaction — reconstructs the full detail of a single transaction.

use uuid::Uuid;

use crate::command::input::get_transaction::GetTransactionQuery;
use crate::command::Command;
use crate::command::CommandState;
use crate::domain::entity::{Entity, PropertyValue};
use crate::domain::managed::Managed;
use crate::domain::transaction::{DeletedEntity, TouchedEntity, TransactionDetail};
use crate::domain::tx_meta::{time_from_uuid, TxMeta};
use crate::io::slug::Slug;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::assertion::{AssertionEntry, AssertionKey};
use crate::store::entry::entity::{EntityEntry, EntityKey};
use crate::store::entry::entity_change::{EntityChangeEntry, EntityChangeKey};
use crate::store::entry::managed::{ManagedEntry, ManagedKey};
use crate::store::entry::touched::{TouchedEntry, TouchedKey};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey};
use crate::store::entry::transaction_log::{ChangeItem, TransactionLogEntry, TransactionLogKey};

/// Reconstructs a transaction from storage entries.
pub struct GetTransaction;

impl Command for GetTransaction {
    type Input = GetTransactionQuery;
    type Output = TransactionDetail;

    fn execute(
        &self,
        branch: &BranchContext,
        _state: &mut CommandState,
        input: Self::Input,
    ) -> Result<Self::Output, DbError> {
        let tx_id = match input.tx_id {
            Some(id) => id,
            None => branch
                .head_tx()?
                .ok_or_else(|| DbError::Storage("no transactions on this branch".into()))?,
        };

        // Load transaction log (global, not branch-scoped — cross-branch by design).
        let log = branch
            .storage()
            .get::<TransactionLogEntry>(&TransactionLogKey { tx_id })?
            .ok_or_else(|| DbError::Storage(format!("transaction not found: {tx_id}")))?;

        let log_branch = log.value.branch.clone();

        if input.only_current_branch && log_branch != *branch.id() {
            return Err(DbError::Storage(format!(
                "transaction {} belongs to branch \"{}\", not current branch \"{}\"",
                tx_id,
                log_branch,
                branch.id()
            )));
        }

        // Load transaction metadata (branch-scoped).
        let tx_entry = branch
            .storage()
            .get::<TransactionEntry>(&TransactionKey {
                branch: log_branch.clone(),
                tx_id,
            })?
            .ok_or_else(|| DbError::Storage(format!("transaction entry not found: {tx_id}")))?;

        let tx_meta = TxMeta {
            tx_id,
            branch: log_branch.clone(),
            reasoning: None,
            time: time_from_uuid(tx_id),
        };

        // Reconstruct created entities.
        let created = log
            .value
            .created
            .iter()
            .map(|item| self.reconstruct_entity(branch, &log_branch, tx_id, item))
            .collect::<Result<Vec<_>, _>>()?;

        // Reconstruct updated entities.
        let updated = log
            .value
            .updated
            .iter()
            .map(|item| self.reconstruct_entity(branch, &log_branch, tx_id, item))
            .collect::<Result<Vec<_>, _>>()?;

        // Reconstruct deleted entities.
        let deleted = log
            .value
            .deleted
            .iter()
            .map(|item| self.reconstruct_deleted(branch, &log_branch, tx_id, item))
            .collect::<Result<Vec<_>, _>>()?;

        // Reconstruct touched entities.
        let touched = log
            .value
            .touched
            .iter()
            .map(|slug| self.reconstruct_touched(branch, &log_branch, tx_id, slug))
            .collect::<Result<Vec<_>, _>>()?;

        // Reconstruct created managed items.
        let created_managed = log
            .value
            .created_managed
            .iter()
            .map(|item| {
                self.reconstruct_managed(branch, &log_branch, tx_id, &item.type_name, &item.slug)
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Reconstruct updated managed items.
        let updated_managed = log
            .value
            .updated_managed
            .iter()
            .map(|item| {
                self.reconstruct_managed(branch, &log_branch, tx_id, &item.type_name, &item.slug)
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(TransactionDetail {
            meta: tx_entry.value.meta,
            branch: log_branch,
            context: tx_meta,
            created,
            updated,
            deleted,
            touched,
            created_managed,
            updated_managed,
        })
    }
}

impl GetTransaction {
    /// Reconstruct an entity from a ChangeItem (for created or updated entities).
    fn reconstruct_entity(
        &self,
        branch: &BranchContext,
        log_branch: &Slug,
        tx_id: Uuid,
        item: &ChangeItem,
    ) -> Result<Entity<TxMeta>, DbError> {
        // Load entity change entry for meta (nil = no properties changed).
        let meta = if item.change_id.is_nil() {
            serde_json::Map::new()
        } else {
            branch
                .storage()
                .get::<EntityChangeEntry>(&EntityChangeKey {
                    change_id: item.change_id,
                })?
                .map(|c| c.value.meta)
                .unwrap_or_default()
        };

        // Load entity record at this tx for description.
        let entity_entry = branch.storage().get::<EntityEntry>(&EntityKey {
            branch: log_branch.clone(),
            entity: item.entity.clone(),
            tx_id,
        })?;
        let description = entity_entry.and_then(|e| e.value.description);

        // Load assertions for the properties changed in this tx.
        let mut properties = Vec::new();
        for prop_slug in &item.properties {
            let assertion = branch.storage().get::<AssertionEntry>(&AssertionKey {
                branch: log_branch.clone(),
                entity: item.entity.clone(),
                prop: prop_slug.clone(),
                tx_id,
            })?;
            if let Some(a) = assertion {
                properties.push(PropertyValue {
                    property: prop_slug.clone(),
                    value: a.value.value,
                    context: TxMeta {
                        tx_id,
                        branch: log_branch.clone(),
                        reasoning: Some(a.value.reasoning),
                        time: time_from_uuid(tx_id),
                    },
                });
            }
        }

        Ok(Entity {
            slug: item.entity.clone(),
            description,
            properties,
            meta,
            context: TxMeta {
                tx_id,
                branch: log_branch.clone(),
                reasoning: None,
                time: time_from_uuid(tx_id),
            },
        })
    }

    /// Reconstruct a deleted entity from a ChangeItem.
    fn reconstruct_deleted(
        &self,
        branch: &BranchContext,
        log_branch: &Slug,
        tx_id: Uuid,
        item: &ChangeItem,
    ) -> Result<DeletedEntity, DbError> {
        // Load EntityChangeEntry for meta (reasoning etc.)
        let meta = if item.change_id.is_nil() {
            serde_json::Map::new()
        } else {
            branch
                .storage()
                .get::<EntityChangeEntry>(&EntityChangeKey {
                    change_id: item.change_id,
                })?
                .map(|c| c.value.meta)
                .unwrap_or_default()
        };

        let reasoning = meta
            .get("reasoning")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        Ok(DeletedEntity {
            slug: item.entity.clone(),
            meta,
            reasoning,
            context: TxMeta {
                tx_id,
                branch: log_branch.clone(),
                reasoning: None,
                time: time_from_uuid(tx_id),
            },
        })
    }

    /// Reconstruct a touched entity.
    fn reconstruct_touched(
        &self,
        branch: &BranchContext,
        log_branch: &Slug,
        tx_id: Uuid,
        slug: &Slug,
    ) -> Result<TouchedEntity, DbError> {
        let touched = branch.storage().get::<TouchedEntry>(&TouchedKey {
            branch: log_branch.clone(),
            tx_id,
            entity: slug.clone(),
        })?;

        let reasoning = touched.map(|t| t.value.reasoning).unwrap_or_default();

        Ok(TouchedEntity {
            slug: slug.clone(),
            reasoning,
        })
    }

    /// Reconstruct a managed item.
    fn reconstruct_managed(
        &self,
        branch: &BranchContext,
        log_branch: &Slug,
        tx_id: Uuid,
        type_name: &Slug,
        slug: &Slug,
    ) -> Result<Managed<TxMeta>, DbError> {
        let entry = branch.storage().get::<ManagedEntry>(&ManagedKey {
            branch: log_branch.clone(),
            type_name: type_name.clone(),
            item: slug.clone(),
            tx_id,
        })?;

        match entry {
            Some(e) => Ok(Managed {
                type_name: type_name.clone(),
                slug: slug.clone(),
                state: e.value.state,
                fields: e.value.fields,
                context: TxMeta {
                    tx_id,
                    branch: log_branch.clone(),
                    reasoning: None,
                    time: time_from_uuid(tx_id),
                },
            }),
            None => Err(DbError::Storage(format!(
                "managed item not found: {type_name}/{slug} at tx {tx_id}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indoc::indoc;
    use serde_json::{Map, Value};

    use super::*;
    use crate::command::executor::checkout::ExecuteCheckout;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::checkout::CheckoutInput;
    use crate::command::input::transaction::{DeleteItem, TransactionInput};
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::PropertyValue as PV;
    use crate::domain::managed::Managed as ManagedDomain;
    use crate::domain::transaction::Transaction;
    use crate::domain::validator::DomainValidator;
    use crate::store::storage::Storage;

    fn test_config() -> Arc<ProjectConfig> {
        Arc::new(
            ProjectConfig::builder()
                .data_dir("./data".into())
                .schema_path("./schema.yaml".into())
                .build(),
        )
    }

    fn test_schema() -> Arc<DataSchema> {
        Arc::new(
            DataSchema::from_yaml(indoc! {"
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
            "})
            .unwrap(),
        )
    }

    fn meta(r: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(r.into()));
        m
    }

    fn exec_tx(branch: &BranchContext, input: TransactionInput) -> Transaction<TxMeta> {
        let cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(branch.storage());
        let result = cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
        result
    }

    fn get_tx(branch: &BranchContext, tx_id: Option<Uuid>) -> TransactionDetail {
        let cmd = GetTransaction;
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, GetTransactionQuery::new(tx_id))
            .unwrap()
    }

    #[test]
    fn get_latest_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec_tx(&branch, TransactionInput::new(meta("first")));
        let tx2 = exec_tx(
            &branch,
            TransactionInput::new(meta("second")).create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(25),
                    context: (),
                }],
                meta("initial"),
            )),
        );

        let detail = get_tx(&branch, None);
        assert_eq!(detail.context.tx_id, tx2.context.tx_id);
        assert_eq!(detail.meta["reasoning"], "second");
        assert_eq!(detail.created.len(), 1);
        assert_eq!(detail.created[0].slug.as_str(), "alice");
        assert_eq!(detail.created[0].properties[0].value, serde_json::json!(25));
        assert_eq!(
            detail.created[0].description,
            Some(serde_json::json!("A person"))
        );
    }

    #[test]
    fn get_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec_tx(
            &branch,
            TransactionInput::new(meta("create")).create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(25),
                    context: (),
                }],
                meta("initial"),
            )),
        );

        let tx2 = exec_tx(
            &branch,
            TransactionInput::new(meta("update")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(26),
                    context: (),
                }],
                meta("birthday"),
            )),
        );

        let detail = get_tx(&branch, Some(tx2.context.tx_id));
        assert_eq!(detail.meta["reasoning"], "update");
        assert!(detail.created.is_empty());
        assert_eq!(detail.updated.len(), 1);
        assert_eq!(detail.updated[0].slug.as_str(), "alice");
        assert_eq!(detail.updated[0].properties[0].value, serde_json::json!(26));
        assert_eq!(detail.updated[0].meta["reasoning"], "birthday");
    }

    #[test]
    fn get_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let result = GetTransaction.execute(
            &branch,
            &mut CommandState::new(&storage),
            GetTransactionQuery::new(Some(Uuid::now_v7())),
        );
        assert!(result.is_err());
    }

    #[test]
    fn get_with_managed() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("investigate"));

        let tx = exec_tx(
            &branch,
            TransactionInput::new(meta("manage")).create_managed(ManagedDomain::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            )),
        );

        let detail = get_tx(&branch, Some(tx.context.tx_id));
        assert_eq!(detail.created_managed.len(), 1);
        assert_eq!(detail.created_managed[0].type_name.as_str(), "task");
        assert_eq!(detail.created_managed[0].slug.as_str(), "task-1");
        assert_eq!(detail.created_managed[0].state.as_deref(), Some("open"));
        assert_eq!(detail.created_managed[0].fields["goal"], "investigate");
    }

    #[test]
    fn get_with_delete() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec_tx(
            &branch,
            TransactionInput::new(meta("create")).create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            )),
        );

        let tx = exec_tx(
            &branch,
            TransactionInput::new(meta("remove")).delete_entity(DeleteItem::new(
                "alice".parse().unwrap(),
                serde_json::json!("no longer relevant"),
            )),
        );

        let detail = get_tx(&branch, Some(tx.context.tx_id));
        assert_eq!(detail.deleted.len(), 1);
        assert_eq!(detail.deleted[0].slug.as_str(), "alice");
        assert_eq!(detail.deleted[0].reasoning, "no longer relevant");
    }

    #[test]
    fn get_cross_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec_tx(&main, TransactionInput::new(meta("seed")));

        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut cs = CommandState::new(&storage);
        let checkout_result = checkout_cmd
            .execute(
                &main,
                &mut cs,
                CheckoutInput::new(
                    "feature".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("on feature")).create_entity(Entity::new(
                        "bob".parse().unwrap(),
                        Some(serde_json::json!("A person")),
                        vec![PV {
                            property: "role".parse().unwrap(),
                            value: serde_json::json!("developer"),
                            context: (),
                        }],
                        meta("setup"),
                    )),
                ),
            )
            .unwrap();
        cs.commit().unwrap();

        let child_tx_id = checkout_result.transaction.context.tx_id;

        // Get from main by explicit tx_id — should still work (global log lookup).
        let detail = get_tx(&main, Some(child_tx_id));
        assert_eq!(detail.branch.as_str(), "feature");
        assert_eq!(detail.created.len(), 1);
        assert_eq!(detail.created[0].slug.as_str(), "bob");
    }

    #[test]
    fn only_current_branch_rejects_foreign_tx() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec_tx(&main, TransactionInput::new(meta("seed")));

        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut cs = CommandState::new(&storage);
        let checkout_result = checkout_cmd
            .execute(
                &main,
                &mut cs,
                CheckoutInput::new(
                    "feature".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("on feature")),
                ),
            )
            .unwrap();
        cs.commit().unwrap();

        let child_tx_id = checkout_result.transaction.context.tx_id;

        // Cross-branch without filter — works.
        let detail = get_tx(&main, Some(child_tx_id));
        assert_eq!(detail.branch.as_str(), "feature");

        // With only_current_branch — rejects.
        let cmd = GetTransaction;
        let mut state = CommandState::new(&storage);
        let err = cmd
            .execute(
                &main,
                &mut state,
                GetTransactionQuery::new(Some(child_tx_id)).only_current_branch(),
            )
            .unwrap_err();
        assert!(err.to_string().contains("belongs to branch"));
        assert!(err.to_string().contains("feature"));
    }
}
