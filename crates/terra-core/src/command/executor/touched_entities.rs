//! ListTouchedEntities — lists recently touched entities with their properties.

use std::collections::HashSet;
use std::sync::Arc;

use uuid::Uuid;

use crate::command::input::touched_entities::TouchedEntitiesQuery;
use crate::command::Command;
use crate::command::CommandState;
use crate::config::DataSchema;
use crate::domain::entity::Entity;
use crate::domain::tx_meta::TxMeta;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::touched::{TouchedEntry, TouchedKey};
use crate::store::query::entity_snapshot;

/// Lists recently touched entities ordered by touch recency (most recent first).
pub struct ListTouchedEntities {
    schema: Arc<DataSchema>,
}

impl ListTouchedEntities {
    /// Create the executor with the project schema (for assertion-status layering).
    pub fn new(schema: Arc<DataSchema>) -> Self {
        Self { schema }
    }
}

impl Command for ListTouchedEntities {
    type Input = TouchedEntitiesQuery;
    type Output = Vec<Entity<TxMeta>>;

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

        let slugs = Self::collect_touched_slugs(branch, at_tx, input.limit)?;
        self.collect_entities(branch, &slugs, at_tx)
    }
}

impl ListTouchedEntities {
    /// Scan touched log backward from at_tx, collect unique entity slugs up to limit.
    fn collect_touched_slugs(
        branch: &BranchContext,
        at_tx: Uuid,
        limit: usize,
    ) -> Result<Vec<Slug>, DbError> {
        let mut seen = HashSet::new();
        let mut result = Vec::new();

        // Current branch.
        Self::scan_touched_on_branch(
            branch.storage(),
            branch.id(),
            at_tx,
            limit,
            &mut seen,
            &mut result,
        )?;

        // Ancestry.
        for ancestor in branch.ancestry() {
            if result.len() >= limit {
                break;
            }
            Self::scan_touched_on_branch(
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
        let statuses = self.schema.assertion_statuses.as_ref();
        let mut entities = Vec::with_capacity(slugs.len());
        for slug in slugs {
            if let Some(entity) =
                entity_snapshot::entity_snapshot(branch, slug, Some(at_tx), statuses)?
            {
                entities.push(entity);
            }
        }
        Ok(entities)
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use serde_json::{Map, Value};
    use std::sync::Arc;

    use super::*;
    use crate::command::executor::checkout::ExecuteCheckout;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::checkout::CheckoutInput;
    use crate::command::input::transaction::{TouchItem, TransactionInput};
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::PropertyValue as PV;
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

    fn exec_tx(branch: &BranchContext, input: TransactionInput) {
        let cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
    }

    fn query(branch: &BranchContext, limit: usize) -> Vec<Entity<TxMeta>> {
        let cmd = ListTouchedEntities::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, TouchedEntitiesQuery::new(None, limit))
            .unwrap()
    }

    #[test]
    fn basic_touched_entities() {
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

        let entities = query(&branch, 10);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].slug.as_str(), "alice");
        assert_eq!(entities[0].properties.len(), 1);
        assert_eq!(entities[0].properties[0].value, serde_json::json!(25));
    }

    #[test]
    fn touch_limit_respected() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        for name in ["alice", "bob", "charlie"] {
            exec_tx(
                &branch,
                TransactionInput::new(meta("create")).create_entity(Entity::new(
                    name.parse().unwrap(),
                    Some(serde_json::json!("person")),
                    vec![],
                    Map::new(),
                )),
            );
        }

        let entities = query(&branch, 2);
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].slug.as_str(), "charlie");
        assert_eq!(entities[1].slug.as_str(), "bob");
    }

    #[test]
    fn explicit_touch_without_mutation_included() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec_tx(
            &branch,
            TransactionInput::new(meta("create")).create_entity(Entity::new(
                "server".parse().unwrap(),
                Some(serde_json::json!("A server")),
                vec![PV {
                    property: "status".parse().unwrap(),
                    value: serde_json::json!("up"),
                    context: (),
                }],
                meta("initial"),
            )),
        );

        exec_tx(
            &branch,
            TransactionInput::new(meta("observe"))
                .touch(TouchItem::new("server".parse().unwrap(), "checked health")),
        );

        let entities = query(&branch, 10);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].slug.as_str(), "server");
        assert_eq!(entities[0].properties[0].value, serde_json::json!("up"));
    }

    #[test]
    fn inherits_from_parent_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec_tx(
            &main,
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

        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut cs = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut cs,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("touch alice on child"))
                        .touch(TouchItem::new("alice".parse().unwrap(), "reviewing")),
                ),
            )
            .unwrap();
        cs.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let entities = query(&child, 10);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].slug.as_str(), "alice");
        assert_eq!(entities[0].properties[0].value, serde_json::json!(25));
    }

    #[test]
    fn property_reasoning_from_assertion() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec_tx(
            &branch,
            TransactionInput::new(meta("create")).create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV {
                        property: "age".parse().unwrap(),
                        value: serde_json::json!(25),
                        context: (),
                    },
                    PV {
                        property: "city".parse().unwrap(),
                        value: serde_json::json!("London"),
                        context: (),
                    },
                ],
                meta("census data"),
            )),
        );

        let entities = query(&branch, 10);
        assert_eq!(entities[0].properties.len(), 2);
        for prop in &entities[0].properties {
            assert_eq!(prop.context.reasoning.as_deref(), Some("census data"));
        }
    }

    #[test]
    fn provenance_branch_from_record_not_current() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        // Create entity on main.
        exec_tx(
            &main,
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

        // Checkout child, touch alice.
        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut cs = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut cs,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("touch alice"))
                        .touch(TouchItem::new("alice".parse().unwrap(), "reviewing")),
                ),
            )
            .unwrap();
        cs.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let entities = query(&child, 10);
        assert_eq!(entities.len(), 1);
        // Entity record lives on main — provenance should reflect that.
        assert_eq!(entities[0].context.branch.as_str(), "main");
    }

    #[test]
    fn provenance_branch_from_current_when_created_here() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec_tx(&main, TransactionInput::new(meta("seed")));

        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut cs = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut cs,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("create on child")).create_entity(Entity::new(
                        "bob".parse().unwrap(),
                        Some(serde_json::json!("A person")),
                        vec![],
                        serde_json::Map::new(),
                    )),
                ),
            )
            .unwrap();
        cs.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let entities = query(&child, 10);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].context.branch.as_str(), "child");
    }
}
