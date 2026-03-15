//! ListManaged — lists managed items in visible lifecycle states.

use std::collections::HashSet;
use std::sync::Arc;

use uuid::Uuid;

use crate::command::Command;
use crate::command::CommandState;
use crate::command::input::list_managed::ListManagedQuery;
use crate::config::{DataSchema, ManagedTypeDef};
use crate::domain::managed::Managed;
use crate::domain::tx_meta::TxMeta;
use crate::io::DbError;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
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
            let type_slug: Slug = type_name.parse()
                .map_err(|e: crate::io::slug::SlugError| DbError::Storage(e.to_string()))?;

            let bound = ManagedKey::bound()
                .with_prefix(|k| {
                    k.branch = branch.id().clone();
                    k.type_name = type_slug.clone();
                });

            // FIXME: inefficient two-pass scan strategy.
            //
            // Current approach: forward scan discovers all item slugs (reading every
            // version of every item — O(N*V)), then does a separate reverse
            // `get_latest` seek per item to find the latest version within at_tx.
            //
            // Better approach: single reverse scan with seek-to-skip, same pattern
            // as `BranchContext::collect_props` / `collect_embeddings`. A reverse
            // scan naturally lands on the latest version first, so we read one
            // entry per item — O(N) seeks total instead of O(N*V) forward reads
            // plus O(N) reverse seeks. The same applies to the ancestry loop below.
            let mut iter = branch.storage().scan::<ManagedEntry>(&bound)?;
            let mut seen_items = HashSet::new();

            loop {
                let entry = match iter.next() {
                    Some(Ok(e)) => e,
                    Some(Err(e)) => return Err(e),
                    None => break,
                };

                let item_slug = entry.key.item.clone();
                if seen_items.contains(&item_slug) {
                    let skip = ManagedKey::bound()
                        .with_prefix(|k| {
                            k.branch = branch.id().clone();
                            k.type_name = type_slug.clone();
                            k.item = item_slug.clone();
                            k.tx_id = Uuid::max();
                        });
                    iter.seek(&skip);
                    continue;
                }
                seen_items.insert(item_slug.clone());

                // Get latest version within tx bound.
                let item_bound = ManagedKey::bound()
                    .with_prefix(|k| {
                        k.branch = branch.id().clone();
                        k.type_name = type_slug.clone();
                        k.item = item_slug.clone();
                    })
                    .with_upper(|k| k.tx_id = at_tx);

                let latest = match branch.get_latest::<ManagedEntry>(&item_bound)? {
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
                    },
                });
            }

            // Walk ancestry for this managed type.
            for ancestor in branch.ancestry() {
                let ancestor_bound = ManagedKey::bound()
                    .with_prefix(|k| {
                        k.branch = ancestor.branch.clone();
                        k.type_name = type_slug.clone();
                    });

                let mut ancestor_iter = branch.storage().scan::<ManagedEntry>(&ancestor_bound)?;

                loop {
                    let entry = match ancestor_iter.next() {
                        Some(Ok(e)) => e,
                        Some(Err(e)) => return Err(e),
                        None => break,
                    };

                    let item_slug = entry.key.item.clone();
                    if seen_items.contains(&item_slug) {
                        let skip = ManagedKey::bound()
                            .with_prefix(|k| {
                                k.branch = ancestor.branch.clone();
                                k.type_name = type_slug.clone();
                                k.item = item_slug.clone();
                                k.tx_id = Uuid::max();
                            });
                        ancestor_iter.seek(&skip);
                        continue;
                    }
                    seen_items.insert(item_slug.clone());

                    let item_bound = ManagedKey::bound()
                        .with_prefix(|k| {
                            k.branch = ancestor.branch.clone();
                            k.type_name = type_slug.clone();
                            k.item = item_slug.clone();
                        })
                        .with_upper(|k| k.tx_id = ancestor.branch_point_tx);

                    let latest = match branch.storage().get_latest::<ManagedEntry>(&item_bound)? {
                        Some(e) => e,
                        None => continue,
                    };

                    if let Some(lc) = &type_def.lifecycle {
                        if !lc.visible.is_empty() {
                            let state = latest.value.state.as_deref().unwrap_or("");
                            if !lc.visible.iter().any(|s| s == state) {
                                continue;
                            }
                        }
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
                        },
                    });
                }
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
    fn managed_visible_only() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("open task"));

        exec_tx(&branch, TransactionInput::new(meta("setup"))
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
            )));

        let cmd = ListManaged::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        let managed = cmd.execute(&branch, &mut state, ListManagedQuery::new(None)).unwrap();
        assert_eq!(managed.len(), 1);
        assert_eq!(managed[0].slug.as_str(), "task-open");
    }
}
