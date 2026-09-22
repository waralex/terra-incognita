//! GetEntity — reads a single entity snapshot by slug.

use std::sync::Arc;

use crate::command::input::entity_get::EntityGetQuery;
use crate::command::Command;
use crate::command::CommandState;
use crate::config::DataSchema;
use crate::domain::entity::Entity;
use crate::domain::tx_meta::TxMeta;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::query::entity_snapshot::entity_snapshot_selected;

/// Reads a single entity snapshot, with assertion-status layering when configured.
pub struct GetEntity {
    schema: Arc<DataSchema>,
}

impl GetEntity {
    /// Create the executor with the project schema (for assertion-status layering).
    pub fn new(schema: Arc<DataSchema>) -> Self {
        Self { schema }
    }
}

impl Command for GetEntity {
    type Input = EntityGetQuery;
    type Output = Entity<TxMeta>;

    fn execute(
        &self,
        branch: &BranchContext,
        _state: &mut CommandState,
        input: Self::Input,
    ) -> Result<Self::Output, DbError> {
        let statuses = self.schema.assertion_statuses.as_ref();
        let entity = entity_snapshot_selected(
            branch,
            &input.entity,
            input.at_tx,
            statuses,
            input.property_prefix.as_ref(),
            input.property_depth,
        )?
        .ok_or_else(|| DbError::Storage(format!("entity not found: {}", input.entity)))?;
        Ok(entity)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use indoc::indoc;
    use serde_json::{Map, Value};

    use super::*;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::transaction::{DeleteItem, TransactionInput};
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::{Entity as DomainEntity, PropertyValue as PV};
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
                  reasoning: { type: text, required: true }
                entity_change_meta:
                  reasoning: { type: text, required: true }
                  source: { type: text }
                branch_meta:
                  reasoning: { type: text, required: true }
                assertion_statuses:
                  values: [fact, hypothesis, observation]
                  terminal: fact
                  default: observation
            "})
            .unwrap(),
        )
    }

    fn meta(r: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(r.into()));
        m
    }

    fn exec_tx(branch: &BranchContext, input: TransactionInput) -> uuid::Uuid {
        let cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(branch.storage());
        let tx = cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
        tx.context.tx_id
    }

    fn get(branch: &BranchContext, slug: &str) -> Result<Entity<TxMeta>, DbError> {
        let cmd = GetEntity::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        cmd.execute(
            branch,
            &mut state,
            EntityGetQuery::new(slug.parse().unwrap()),
        )
    }

    fn get_at(branch: &BranchContext, slug: &str, at_tx: uuid::Uuid) -> Entity<TxMeta> {
        let cmd = GetEntity::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        cmd.execute(
            branch,
            &mut state,
            EntityGetQuery::new(slug.parse().unwrap()).with_at_tx(at_tx),
        )
        .unwrap()
    }

    #[test]
    fn subtree_preserves_layering_and_historical_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let mut summary_tx = None;
        for (status, value) in [
            ("observation", "old"),
            ("fact", "summary"),
            ("hypothesis", "new"),
        ] {
            let tx = exec_tx(
                &branch,
                TransactionInput::new(meta(value)).write_entity(
                    DomainEntity::new(
                        "cube".parse().unwrap(),
                        Some(Value::String("test".into())),
                        vec![
                            PV {
                                supersedes_tx: None,
                                property: "a.b".parse().unwrap(),
                                value: Value::String(value.into()),
                                context: (),
                            },
                            PV {
                                supersedes_tx: None,
                                property: "ab".parse().unwrap(),
                                value: Value::String("sibling".into()),
                                context: (),
                            },
                        ],
                        meta(value),
                    )
                    .with_status(Some(status.into())),
                ),
            );
            if status == "fact" {
                summary_tx = Some(tx);
            }
        }
        let cmd = GetEntity::new(test_schema());
        let query = || {
            EntityGetQuery::new("cube".parse().unwrap()).with_property_prefix("a".parse().unwrap())
        };
        let mut state = CommandState::new(branch.storage());
        let current = cmd.execute(&branch, &mut state, query()).unwrap();
        let tree = crate::domain::property_tree::property_tree(current.properties);
        let assertions = &tree["a"].children["b"].assertions;
        assert_eq!(assertions.len(), 2);
        assert_eq!(assertions[0].value, "summary");
        assert_eq!(assertions[1].context.status.as_deref(), Some("hypothesis"));
        let historical = cmd
            .execute(&branch, &mut state, query().with_at_tx(summary_tx.unwrap()))
            .unwrap();
        assert_eq!(historical.properties.len(), 1);
        assert_eq!(historical.properties[0].value, "summary");
    }

    #[test]
    fn depth_refs_respect_snapshot_and_can_be_opened() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let write = |paths: &[&str]| {
            exec_tx(
                &branch,
                TransactionInput::new(meta("seed")).write_entity(
                    DomainEntity::new(
                        "cube".parse().unwrap(),
                        Some(serde_json::json!("test")),
                        paths
                            .iter()
                            .map(|p| PV {
                                supersedes_tx: None,
                                property: p.parse().unwrap(),
                                value: serde_json::json!(p),
                                context: (),
                            })
                            .collect(),
                        meta("seed"),
                    )
                    .with_status(Some("fact".into())),
                ),
            )
        };
        let before = write(&["a", "a.b", "a.b.c", "ab.c", "odd.b-.c"]);
        write(&["a.future.deep"]);
        let cmd = GetEntity::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        let q = || {
            EntityGetQuery::new("cube".parse().unwrap())
                .with_property_prefix("a".parse().unwrap())
                .with_property_depth(1)
                .with_at_tx(before)
        };
        let result = cmd.execute(&branch, &mut state, q()).unwrap();
        assert_eq!(
            result
                .properties
                .iter()
                .map(|p| p.property.as_str())
                .collect::<Vec<_>>(),
            ["a", "a.b"]
        );
        assert_eq!(
            result
                .property_refs
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>(),
            ["a.b"]
        );
        let opened = cmd
            .execute(
                &branch,
                &mut state,
                EntityGetQuery::new("cube".parse().unwrap())
                    .with_property_prefix(result.property_refs[0].clone())
                    .with_property_depth(1)
                    .with_at_tx(before),
            )
            .unwrap();
        assert_eq!(opened.properties.len(), 2);
        assert!(opened.property_refs.is_empty());
        let exact = cmd
            .execute(&branch, &mut state, q().with_property_depth(0))
            .unwrap();
        assert_eq!(exact.properties.len(), 1);
        assert_eq!(exact.property_refs[0].as_str(), "a");
        let odd = cmd
            .execute(
                &branch,
                &mut state,
                EntityGetQuery::new("cube".parse().unwrap())
                    .with_property_prefix("odd".parse().unwrap())
                    .with_property_depth(1),
            )
            .unwrap();
        assert_eq!(odd.property_refs[0].as_str(), "odd.b-.c");
        assert!(odd.property_refs[0]
            .as_str()
            .parse::<crate::io::Slug>()
            .is_ok());
    }

    #[test]
    fn get_existing_entity() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let mut change = meta("user told me");
        change.insert("source".into(), Value::String("user".into()));
        exec_tx(
            &branch,
            TransactionInput::new(meta("seed")).write_entity(DomainEntity::new(
                "cube".parse().unwrap(),
                Some(serde_json::json!("Cube.js project")),
                vec![PV {
                    supersedes_tx: None,
                    property: "language".parse().unwrap(),
                    value: serde_json::json!("TypeScript"),
                    context: (),
                }],
                change,
            )),
        );

        let entity = get(&branch, "cube").unwrap();
        assert_eq!(entity.slug.as_str(), "cube");
        assert_eq!(
            entity.description,
            Some(serde_json::json!("Cube.js project"))
        );
        assert_eq!(entity.properties.len(), 1);
        let prop = &entity.properties[0];
        assert_eq!(prop.value, serde_json::json!("TypeScript"));
        assert_eq!(prop.context.source.as_deref(), Some("user"));
        assert_eq!(prop.context.status.as_deref(), Some("observation"));
    }

    #[test]
    fn get_at_tx_returns_past_state() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let tx1 = exec_tx(
            &branch,
            TransactionInput::new(meta("seed")).write_entity(DomainEntity::new(
                "cube".parse().unwrap(),
                Some(serde_json::json!("Cube.js project")),
                vec![PV {
                    supersedes_tx: None,
                    property: "language".parse().unwrap(),
                    value: serde_json::json!("TypeScript"),
                    context: (),
                }],
                meta("created"),
            )),
        );
        exec_tx(
            &branch,
            TransactionInput::new(meta("grow")).write_entity(DomainEntity::new(
                "cube".parse().unwrap(),
                None,
                vec![PV {
                    supersedes_tx: None,
                    property: "stars".parse().unwrap(),
                    value: serde_json::json!(100),
                    context: (),
                }],
                meta("added stars"),
            )),
        );

        // As of tx1: only `language`, no `stars`.
        let past = get_at(&branch, "cube", tx1);
        let past_props: Vec<&str> = past
            .properties
            .iter()
            .map(|p| p.property.as_str())
            .collect();
        assert_eq!(past_props, vec!["language"]);

        // Latest: both properties present.
        let now = get(&branch, "cube").unwrap();
        assert_eq!(now.properties.len(), 2);
    }

    #[test]
    fn get_missing_entity_errors_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let err = get(&branch, "ghost").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn get_deleted_entity_errors_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec_tx(
            &branch,
            TransactionInput::new(meta("seed")).write_entity(DomainEntity::new(
                "tmp".parse().unwrap(),
                Some(serde_json::json!("temporary")),
                vec![],
                meta("created"),
            )),
        );
        exec_tx(
            &branch,
            TransactionInput::new(meta("remove")).delete_entity(DeleteItem::new(
                "tmp".parse().unwrap(),
                serde_json::json!("no longer relevant"),
            )),
        );

        let err = get(&branch, "tmp").unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
