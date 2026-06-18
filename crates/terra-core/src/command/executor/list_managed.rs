//! ListManaged — lists managed items in visible lifecycle states.

use std::collections::HashSet;
use std::sync::Arc;

use uuid::Uuid;

use crate::command::input::list_managed::ListManagedQuery;
use crate::command::Command;
use crate::command::CommandState;
use crate::config::{DataSchema, ManagedTypeDef};
use crate::domain::managed::Managed;
use crate::domain::tx_meta::{time_from_uuid, TxMeta};
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::managed::{ManagedEntry, ManagedKey};

fn is_lifecycle_visible(type_def: &ManagedTypeDef, state: Option<&str>) -> bool {
    match &type_def.lifecycle {
        Some(lc) if !lc.visible.is_empty() => {
            let s = state.unwrap_or("");
            lc.visible.iter().any(|v| v == s)
        }
        _ => true,
    }
}

/// Collect managed items on a single branch scope via forward scan with seek-to-skip.
/// Discovers item slugs, then does a bounded reverse seek per item.
fn collect_managed(
    storage: &crate::store::storage::Storage,
    type_slug: &Slug,
    on_branch: &Slug,
    at_tx: Option<Uuid>,
    type_def: &ManagedTypeDef,
    seen_items: &mut HashSet<Slug>,
    result: &mut Vec<Managed<TxMeta>>,
) -> Result<(), DbError> {
    let mut bound = ManagedKey::bound().with_prefix(|k| {
        k.branch = on_branch.clone();
        k.type_name = type_slug.clone();
    });
    if let Some(tx) = at_tx {
        bound = bound.with_upper(|k| k.tx_id = tx);
    }

    let mut iter = storage.scan::<ManagedEntry>(&bound)?;

    loop {
        let entry = match iter.next() {
            Some(Ok(e)) => e,
            Some(Err(e)) => return Err(e),
            None => break,
        };

        let item_slug = entry.key.item.clone();

        if seen_items.contains(&item_slug) {
            let skip = ManagedKey::bound().with_prefix(|k| {
                k.branch = on_branch.clone();
                k.type_name = type_slug.clone();
                k.item = item_slug.clone();
                k.tx_id = Uuid::max();
            });
            iter.seek(&skip);
            continue;
        }
        seen_items.insert(item_slug.clone());

        let mut item_bound = ManagedKey::bound().with_prefix(|k| {
            k.branch = on_branch.clone();
            k.type_name = type_slug.clone();
            k.item = item_slug.clone();
        });
        if let Some(tx) = at_tx {
            item_bound = item_bound.with_upper(|k| k.tx_id = tx);
        }

        let latest = match storage.get_latest::<ManagedEntry>(&item_bound)? {
            Some(e) => e,
            None => continue,
        };

        if !is_lifecycle_visible(type_def, latest.value.state.as_deref()) {
            continue;
        }

        result.push(Managed {
            type_name: type_slug.clone(),
            slug: item_slug,
            state: latest.value.state,
            fields: latest.value.fields,
            context: TxMeta {
                tx_id: latest.key.tx_id,
                branch: latest.key.branch,
                reasoning: None,
                time: time_from_uuid(latest.key.tx_id),
                status: None,
            },
        });

        let skip = ManagedKey::bound().with_prefix(|k| {
            k.branch = on_branch.clone();
            k.type_name = type_slug.clone();
            k.item = entry.key.item.clone();
            k.tx_id = Uuid::max();
        });
        iter.seek(&skip);
    }

    Ok(())
}

/// Lists managed items filtered by lifecycle visibility, walking ancestry.
pub struct ListManaged {
    schema: Arc<DataSchema>,
}

impl ListManaged {
    pub fn new(schema: Arc<DataSchema>) -> Self {
        Self { schema }
    }
}

impl Command for ListManaged {
    type Input = ListManagedQuery;
    type Output = Vec<Managed<TxMeta>>;

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

        for (type_name, type_def) in &self.schema.managed_types {
            let type_slug: Slug = type_name
                .parse()
                .map_err(|e: crate::io::slug::SlugError| DbError::Storage(e.to_string()))?;

            let mut seen_items = HashSet::new();

            let scopes: Vec<_> = branch.scopes_at(at_tx).collect();
            for scope in &scopes {
                collect_managed(
                    branch.storage(),
                    &type_slug,
                    &scope.branch,
                    scope.upper_tx,
                    type_def,
                    &mut seen_items,
                    &mut result,
                )?;
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use serde_json::{Map, Value};
    use std::sync::Arc;

    use super::*;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::transaction::TransactionInput;
    use crate::config::{DataSchema, ProjectConfig};
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
                  states: [open, closed]
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

    fn exec_tx(branch: &BranchContext, input: TransactionInput) {
        let cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
    }

    #[test]
    fn managed_visible_only() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("open task"));

        exec_tx(
            &branch,
            TransactionInput::new(meta("setup"))
                .create_managed(crate::domain::managed::Managed::new(
                    "task".parse().unwrap(),
                    "task-open".parse().unwrap(),
                    Some("open".into()),
                    fields.clone(),
                ))
                .create_managed(crate::domain::managed::Managed::new(
                    "task".parse().unwrap(),
                    "task-closed".parse().unwrap(),
                    Some("closed".into()),
                    fields,
                )),
        );

        let cmd = ListManaged::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        let managed = cmd
            .execute(&branch, &mut state, ListManagedQuery::new(None))
            .unwrap();
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0].slug.as_str(), "task-open");
    }

    #[test]
    fn managed_inherited_from_parent_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("parent task"));

        exec_tx(
            &main,
            TransactionInput::new(meta("setup")).create_managed(
                crate::domain::managed::Managed::new(
                    "task".parse().unwrap(),
                    "task-from-main".parse().unwrap(),
                    Some("open".into()),
                    fields,
                ),
            ),
        );

        // Checkout child branch.
        let checkout_cmd = crate::command::executor::checkout::ExecuteCheckout::new(
            DomainValidator::new(test_schema()),
        );
        let mut cs = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut cs,
                crate::command::input::checkout::CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("child task")).create_managed(
                        crate::domain::managed::Managed::new(
                            "task".parse().unwrap(),
                            "task-from-child".parse().unwrap(),
                            Some("open".into()),
                            {
                                let mut f = Map::new();
                                f.insert("goal".into(), serde_json::json!("child task"));
                                f
                            },
                        ),
                    ),
                ),
            )
            .unwrap();
        cs.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let cmd = ListManaged::new(test_schema());
        let mut state = CommandState::new(child.storage());
        let managed = cmd
            .execute(&child, &mut state, ListManagedQuery::new(None))
            .unwrap();

        assert_eq!(managed.len(), 2);
        let slugs: Vec<&str> = managed.iter().map(|m| m.slug.as_str()).collect();
        assert!(slugs.contains(&"task-from-main"));
        assert!(slugs.contains(&"task-from-child"));
    }
}
