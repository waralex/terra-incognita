//! CollectState — reads branch state: touched entities, managed items, recent transactions.

use std::collections::HashSet;

use uuid::Uuid;

use crate::command::Command;
use crate::command::CommandState;
use crate::command::input::state::{StateSettings, StateQuery};
use crate::config::DataSchema;
use crate::domain::branch::Branch;
use crate::domain::entity::{Entity, PropertyValue};
use crate::domain::managed::Managed;
use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::TxMeta;
use crate::io::DbError;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::store::branch_context::{BranchContext, main_branch_slug};
use crate::store::entry::branch::{BranchEntry, BranchKey};
use crate::store::entry::entity::{EntityEntry, EntityKey};
use crate::store::entry::managed::{ManagedEntry, ManagedKey};
use crate::store::entry::touched::{TouchedEntry, TouchedKey};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey};

use std::sync::Arc;

/// Collected branch state — the agent's working context.
#[derive(Debug)]
pub struct StateOutput {
    /// Current branch info.
    pub branch: Branch<TxMeta>,
    /// Entities with properties, ordered by touched recency (most recent first).
    pub entities: Vec<Entity<TxMeta>>,
    /// Managed items in visible lifecycle states.
    pub managed: Vec<Managed<TxMeta>>,
    /// Recent transactions (most recent first).
    pub transactions: Vec<Transaction<TxMeta>>,
}

/// Collects the current state of a branch for the agent's context.
pub struct CollectState {
    schema: Arc<DataSchema>,
}

impl CollectState {
    pub fn new(schema: Arc<DataSchema>) -> Self {
        Self { schema }
    }
}

impl Command for CollectState {
    type Input = StateQuery;
    type Output = StateOutput;

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

        let branch_info = self.collect_branch(branch)?;
        let entity_slugs = self.collect_touched_slugs(branch, at_tx, input.settings.touch_limit)?;
        let entities = self.collect_entities(branch, &entity_slugs, at_tx)?;
        let managed = self.collect_managed(branch, at_tx)?;
        let transactions = self.collect_transactions(branch, at_tx, input.settings.last_transaction_limit)?;

        Ok(StateOutput {
            branch: branch_info,
            entities,
            managed,
            transactions,
        })
    }
}

impl CollectState {
    fn collect_branch(&self, branch: &BranchContext) -> Result<Branch<TxMeta>, DbError> {
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

        let parent: Slug = entry.value.parent_branch_slug.parse()
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

    /// Scan touched log backward from at_tx, collect unique entity slugs up to limit.
    fn collect_touched_slugs(
        &self,
        branch: &BranchContext,
        at_tx: Uuid,
        limit: usize,
    ) -> Result<Vec<Slug>, DbError> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();

        // Current branch.
        self.scan_touched_on_branch(branch.storage(), branch.id(), at_tx, limit, &mut seen, &mut result)?;

        // Ancestry.
        for ancestor in branch.ancestry() {
            if result.len() >= limit {
                break;
            }
            self.scan_touched_on_branch(
                branch.storage(),
                &ancestor.branch,
                ancestor.branch_point_tx,
                limit,
                &mut seen,
                &mut result,
            )?;
        }

        Ok(result)
    }

    fn scan_touched_on_branch(
        &self,
        storage: &crate::store::storage::Storage,
        on_branch: &Slug,
        at_tx: Uuid,
        limit: usize,
        seen: &mut HashSet<Slug>,
        result: &mut Vec<Slug>,
    ) -> Result<(), DbError> {
        let bound = TouchedKey::bound()
            .with_prefix(|k| k.branch = on_branch.clone())
            .with_upper(|k| k.tx_id = at_tx);

        let iter = storage.scan_rev::<TouchedEntry>(&bound)?;
        for entry_result in iter {
            if result.len() >= limit {
                break;
            }
            let entry = entry_result?;
            if seen.insert(entry.key.entity.clone()) {
                result.push(entry.key.entity);
            }
        }

        Ok(())
    }

    /// For each entity slug, load entity record + properties → domain Entity.
    fn collect_entities(
        &self,
        branch: &BranchContext,
        slugs: &[Slug],
        at_tx: Uuid,
    ) -> Result<Vec<Entity<TxMeta>>, DbError> {
        let mut entities = Vec::with_capacity(slugs.len());

        for slug in slugs {
            let bound = EntityKey::bound()
                .with_prefix(|k| {
                    k.branch = branch.id().clone();
                    k.entity = slug.clone();
                })
                .with_upper(|k| k.tx_id = at_tx);

            let entry = branch.get_latest::<EntityEntry>(&bound)?;
            let description = entry.as_ref().and_then(|e| e.value.description.clone());
            let entity_tx = entry.as_ref().map(|e| e.key.tx_id).unwrap_or(Uuid::nil());

            let assertion_entries = branch.properties(slug, Some(at_tx))?;
            let properties: Vec<PropertyValue<TxMeta>> = assertion_entries
                .into_iter()
                .map(|a| PropertyValue {
                    property: a.key.prop,
                    value: a.value.value.clone(),
                    context: TxMeta {
                        tx_id: a.key.tx_id,
                        branch: a.key.branch,
                        reasoning: Some(a.value.reasoning),
                    },
                })
                .collect();

            entities.push(Entity {
                slug: slug.clone(),
                description,
                properties,
                meta: serde_json::Map::new(),
                context: TxMeta {
                    tx_id: entity_tx,
                    branch: branch.id().clone(),
                    reasoning: None,
                },
            });
        }

        Ok(entities)
    }

    /// Collect all managed items in visible lifecycle states.
    fn collect_managed(
        &self,
        branch: &BranchContext,
        at_tx: Uuid,
    ) -> Result<Vec<Managed<TxMeta>>, DbError> {
        let mut result = Vec::new();

        for (type_name, type_def) in &self.schema.managed_types {
            let type_slug: Slug = type_name.parse()
                .map_err(|e: crate::io::slug::SlugError| DbError::Storage(e.to_string()))?;

            let bound = ManagedKey::bound()
                .with_prefix(|k| {
                    k.branch = branch.id().clone();
                    k.type_name = type_slug.clone();
                });

            // Forward scan to discover items, get_latest per item.
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
                    // Seek past this item.
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

                // Filter by visible states.
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

            // Also walk ancestry for this managed type.
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

    /// Collect last N transactions by scanning backward from at_tx.
    fn collect_transactions(
        &self,
        branch: &BranchContext,
        at_tx: Uuid,
        limit: usize,
    ) -> Result<Vec<Transaction<TxMeta>>, DbError> {
        let mut result = Vec::new();

        let bound = TransactionKey::bound()
            .with_prefix(|k| k.branch = branch.id().clone())
            .with_upper(|k| k.tx_id = at_tx);

        let iter = branch.storage().scan_rev::<TransactionEntry>(&bound)?;
        for entry_result in iter {
            if result.len() >= limit {
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
            if result.len() >= limit {
                break;
            }
            let ancestor_bound = TransactionKey::bound()
                .with_prefix(|k| k.branch = ancestor.branch.clone())
                .with_upper(|k| k.tx_id = ancestor.branch_point_tx);

            let iter = branch.storage().scan_rev::<TransactionEntry>(&ancestor_bound)?;
            for entry_result in iter {
                if result.len() >= limit {
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
    use crate::command::executor::checkout::ExecuteCheckout;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::checkout::CheckoutInput;
    use crate::command::input::transaction::{TransactionInput, TouchItem};
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::PropertyValue as PV;
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

    fn exec_tx(branch: &BranchContext, input: TransactionInput) -> Transaction<TxMeta> {
        let cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(branch.storage());
        let result = cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
        result
    }

    fn collect(branch: &BranchContext, touch_limit: usize, tx_limit: usize) -> StateOutput {
        let cmd = CollectState::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        let limits = StateSettings { touch_limit, last_transaction_limit: tx_limit };
        cmd.execute(branch, &mut state, StateQuery::new(None, limits)).unwrap()
    }

    #[test]
    fn basic_state_collection() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec_tx(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                ],
                meta("initial"),
            )));

        let output = collect(&branch, 10, 10);
        assert_eq!(output.branch.slug.as_str(), "main");
        assert_eq!(output.entities.len(), 1);
        assert_eq!(output.entities[0].slug.as_str(), "alice");
        assert_eq!(output.entities[0].properties.len(), 1);
        assert_eq!(output.entities[0].properties[0].value, serde_json::json!(25));
        assert_eq!(output.transactions.len(), 1);
    }

    #[test]
    fn touch_limit_respected() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        for name in ["alice", "bob", "charlie"] {
            exec_tx(&branch, TransactionInput::new(meta("create"))
                .create_entity(Entity::new(
                    name.parse().unwrap(),
                    Some(serde_json::json!("person")),
                    vec![],
                    Map::new(),
                )));
        }

        let output = collect(&branch, 2, 10);
        assert_eq!(output.entities.len(), 2);
        // Most recently touched first: charlie, bob
        assert_eq!(output.entities[0].slug.as_str(), "charlie");
        assert_eq!(output.entities[1].slug.as_str(), "bob");
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

        let output = collect(&branch, 10, 10);
        assert_eq!(output.managed.len(), 1);
        assert_eq!(output.managed[0].slug.as_str(), "task-open");
    }

    #[test]
    fn transaction_limit_respected() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        for i in 0..5 {
            exec_tx(&branch, TransactionInput::new(meta(&format!("tx-{}", i))));
        }

        let output = collect(&branch, 10, 3);
        assert_eq!(output.transactions.len(), 3);
        assert_eq!(output.transactions[0].meta["reasoning"], "tx-4");
        assert_eq!(output.transactions[2].meta["reasoning"], "tx-2");
    }

    #[test]
    fn explicit_touch_without_mutation_included() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        // Create entity first.
        exec_tx(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "server".parse().unwrap(),
                Some(serde_json::json!("A server")),
                vec![
                    PV { property: "status".parse().unwrap(), value: serde_json::json!("up"), context: () },
                ],
                meta("initial"),
            )));

        // Touch without mutation.
        exec_tx(&branch, TransactionInput::new(meta("observe"))
            .touch(TouchItem::new("server".parse().unwrap(), "checked health")));

        let output = collect(&branch, 10, 10);
        assert_eq!(output.entities.len(), 1);
        assert_eq!(output.entities[0].slug.as_str(), "server");
        assert_eq!(output.entities[0].properties[0].value, serde_json::json!("up"));
    }

    #[test]
    fn inherits_from_parent_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec_tx(&main, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                ],
                meta("initial"),
            )));

        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut cs = CommandState::new(&storage);
        checkout_cmd.execute(&main, &mut cs, CheckoutInput::new(
            "child".parse().unwrap(),
            meta("explore"),
            None,
            TransactionInput::new(meta("touch alice on child"))
                .touch(TouchItem::new("alice".parse().unwrap(), "reviewing")),
        )).unwrap();
        cs.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let output = collect(&child, 10, 10);
        assert_eq!(output.entities.len(), 1);
        assert_eq!(output.entities[0].slug.as_str(), "alice");
        assert_eq!(output.entities[0].properties[0].value, serde_json::json!(25));
    }

    #[test]
    fn property_reasoning_from_assertion() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec_tx(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                    PV { property: "city".parse().unwrap(), value: serde_json::json!("London"), context: () },
                ],
                meta("census data"),
            )));

        let output = collect(&branch, 10, 10);
        assert_eq!(output.entities[0].properties.len(), 2);
        for prop in &output.entities[0].properties {
            assert_eq!(prop.context.reasoning.as_deref(), Some("census data"));
        }
    }
}
