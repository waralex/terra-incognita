//! ListEntityHistory — retrieves the change history of an entity.

use std::collections::BTreeMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::command::Command;

use crate::command::input::entity_history::EntityHistoryQuery;
use crate::command::CommandState;
use crate::config::{AssertionStatusesDef, DataSchema};
use crate::domain::entity::Entity;
use crate::domain::entity_history::EntityHistoryEntry;
use crate::domain::tx_meta::TxMeta;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::assertion::{AssertionEntry, AssertionKey};
use crate::store::entry::entity::{EntityEntry, EntityKey};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey};
use crate::store::query::entity_snapshot;

/// Lists entity history entries ordered most-recent-first.
pub struct ListEntityHistory {
    schema: Arc<DataSchema>,
}

impl ListEntityHistory {
    /// Create the executor with the project schema (for assertion-status layering).
    pub fn new(schema: Arc<DataSchema>) -> Self {
        Self { schema }
    }
}

impl Command for ListEntityHistory {
    type Input = EntityHistoryQuery;
    type Output = Vec<EntityHistoryEntry>;

    fn execute(
        &self,
        branch: &BranchContext,
        _state: &mut CommandState,
        input: Self::Input,
    ) -> Result<Self::Output, DbError> {
        let entity = &input.entity;

        // Step 1: resolve effective bounds.
        let upper_tx = input
            .tx_id_to
            .or(input.at_tx)
            .or(branch.head_tx()?)
            .unwrap_or(Uuid::nil());

        let lower_tx = input.tx_id_from;

        // Verify entity exists.
        let entity_bound = EntityKey::bound()
            .with_prefix(|k| k.entity = entity.clone())
            .with_upper(|k| k.tx_id = upper_tx);
        if !branch.exists::<EntityEntry>(&entity_bound)? {
            return Err(DbError::Storage(format!("entity not found: {}", entity)));
        }

        // Step 2: discover tx_ids that touched this entity.
        // BTreeMap<tx_id, Vec<changed_property_slugs>>
        let mut tx_changes: BTreeMap<Uuid, Vec<Slug>> = BTreeMap::new();

        let scopes: Vec<_> = branch.scopes_at(upper_tx).collect();
        for scope in &scopes {
            collect_assertion_txs(
                branch.storage(),
                entity,
                input.property.as_ref(),
                &scope.branch,
                scope.upper_tx,
                &mut tx_changes,
            )?;

            if input.property.is_none() {
                collect_entity_txs(
                    branch.storage(),
                    entity,
                    &scope.branch,
                    scope.upper_tx,
                    &mut tx_changes,
                )?;
            }
        }

        // Step 3: apply bounds and limit.
        let selected: Vec<(Uuid, Vec<Slug>)> = tx_changes
            .into_iter()
            .rev()
            .filter(|(tx_id, _)| {
                if let Some(lower) = lower_tx {
                    *tx_id >= lower
                } else {
                    true
                }
            })
            .take(input.limit)
            .collect();

        // Step 4: reconstruct snapshots.
        let statuses = self.schema.assertion_statuses.as_ref();
        let mut entries = Vec::with_capacity(selected.len());
        for (tx_id, changed_props) in selected {
            let entry = build_history_entry(branch, entity, tx_id, changed_props, statuses)?;
            entries.push(entry);
        }

        Ok(entries)
    }
}

/// Scan AssertionEntry to discover tx_ids where properties changed.
///
/// Key layout: `branch | entity | prop | tx_id`. Because prop sorts before tx_id,
/// the KeyBound upper on tx_id doesn't cap per-property — entries with lower prop
/// hashes but higher tx_ids still fall within the range. We filter by tx_id manually.
fn collect_assertion_txs(
    storage: &crate::store::storage::Storage,
    entity: &Slug,
    property_filter: Option<&Slug>,
    on_branch: &Slug,
    at_tx: Option<Uuid>,
    tx_changes: &mut BTreeMap<Uuid, Vec<Slug>>,
) -> Result<(), DbError> {
    let mut bound = AssertionKey::bound().with_prefix(|k| {
        k.branch = on_branch.clone();
        k.entity = entity.clone();
    });
    if let Some(prop) = property_filter {
        bound = bound.with_prefix(|k| k.prop = prop.clone());
    }

    let iter = storage.scan::<AssertionEntry>(&bound)?;
    for entry_result in iter {
        let entry = entry_result?;
        if let Some(upper) = at_tx {
            if entry.key.tx_id > upper {
                continue;
            }
        }
        tx_changes
            .entry(entry.key.tx_id)
            .or_default()
            .push(entry.key.prop);
    }

    Ok(())
}

/// Scan EntityEntry to discover tx_ids where the entity record changed.
///
/// EntityKey layout: `branch | entity | tx_id` — tx_id is the last field,
/// so `with_upper` on tx_id correctly bounds the scan range.
fn collect_entity_txs(
    storage: &crate::store::storage::Storage,
    entity: &Slug,
    on_branch: &Slug,
    at_tx: Option<Uuid>,
    tx_changes: &mut BTreeMap<Uuid, Vec<Slug>>,
) -> Result<(), DbError> {
    let mut bound = EntityKey::bound().with_prefix(|k| {
        k.branch = on_branch.clone();
        k.entity = entity.clone();
    });
    if let Some(tx) = at_tx {
        bound = bound.with_upper(|k| k.tx_id = tx);
    }

    let iter = storage.scan::<EntityEntry>(&bound)?;
    for entry_result in iter {
        let entry = entry_result?;
        tx_changes.entry(entry.key.tx_id).or_default();
    }

    Ok(())
}

/// Build a single EntityHistoryEntry for a given tx_id.
fn build_history_entry(
    branch: &BranchContext,
    slug: &Slug,
    tx_id: Uuid,
    changed_props: Vec<Slug>,
    statuses: Option<&AssertionStatusesDef>,
) -> Result<EntityHistoryEntry, DbError> {
    let entity = entity_snapshot::entity_snapshot(branch, slug, Some(tx_id), statuses)?
        .unwrap_or_else(|| Entity {
            slug: slug.clone(),
            description: None,
            properties: vec![],
            meta: serde_json::Map::new(),
            status: None,
            context: TxMeta {
                tx_id: Uuid::nil(),
                branch: branch.id().clone(),
                reasoning: None,
                time: None,
                status: None,
            },
        });

    let tx_entry = load_transaction_meta(branch, tx_id)?;
    let transaction_meta = tx_entry.map(|e| e.value.meta).unwrap_or_default();

    Ok(EntityHistoryEntry {
        entity,
        changed_properties: changed_props,
        transaction_meta,
    })
}

/// Load transaction metadata, walking ancestry to find it.
fn load_transaction_meta(
    branch: &BranchContext,
    tx_id: Uuid,
) -> Result<Option<TransactionEntry>, DbError> {
    for scope in branch.scopes() {
        let key = TransactionKey {
            branch: scope.branch.clone(),
            tx_id,
        };
        if let Some(entry) = branch.storage().get::<TransactionEntry>(&key)? {
            return Ok(Some(entry));
        }
    }
    Ok(None)
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
    use crate::command::input::transaction::TransactionInput;
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::Entity;
    use crate::domain::entity::PropertyValue as PV;
    use crate::domain::transaction::Transaction;
    use crate::domain::tx_meta::TxMeta;
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
            "})
            .unwrap(),
        )
    }

    fn validator() -> DomainValidator {
        DomainValidator::new(test_schema())
    }

    fn meta(r: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(r.into()));
        m
    }

    fn exec(branch: &BranchContext, input: TransactionInput) -> Transaction<TxMeta> {
        let cmd = ExecuteTransaction::new(validator());
        let mut state = CommandState::new(branch.storage());
        let result = cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
        result
    }

    fn query(branch: &BranchContext, input: EntityHistoryQuery) -> Vec<EntityHistoryEntry> {
        let cmd = ListEntityHistory::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, input).unwrap()
    }

    #[test]
    fn basic_history_create_and_update() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec(
            &branch,
            TransactionInput::new(meta("create alice")).create_entity(Entity::new(
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

        exec(
            &branch,
            TransactionInput::new(meta("update alice")).update_entity(Entity::new(
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

        let history = query(
            &branch,
            EntityHistoryQuery::new("alice".parse().unwrap(), 10),
        );
        assert_eq!(history.len(), 2);

        // Most recent first.
        assert_eq!(
            history[0].changed_properties,
            vec!["age".parse::<Slug>().unwrap()]
        );
        assert_eq!(history[0].entity.properties[0].value, serde_json::json!(26));
        assert_eq!(history[0].transaction_meta["reasoning"], "update alice");

        // First entry has create (entity record + property).
        assert_eq!(history[1].entity.properties[0].value, serde_json::json!(25));
        assert_eq!(history[1].transaction_meta["reasoning"], "create alice");
    }

    #[test]
    fn description_only_change() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec(
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

        exec(
            &branch,
            TransactionInput::new(meta("update desc")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A great person")),
                vec![],
                meta("better description"),
            )),
        );

        let history = query(
            &branch,
            EntityHistoryQuery::new("alice".parse().unwrap(), 10),
        );
        assert_eq!(history.len(), 2);
        // Description-only change has empty changed_properties.
        assert!(history[0].changed_properties.is_empty());
        assert_eq!(
            history[0].entity.description,
            Some(serde_json::json!("A great person"))
        );
    }

    #[test]
    fn property_filter() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec(
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
                meta("initial"),
            )),
        );

        exec(
            &branch,
            TransactionInput::new(meta("update age")).update_entity(Entity::new(
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

        exec(
            &branch,
            TransactionInput::new(meta("update city")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "city".parse().unwrap(),
                    value: serde_json::json!("Paris"),
                    context: (),
                }],
                meta("moved"),
            )),
        );

        let history = query(
            &branch,
            EntityHistoryQuery::new("alice".parse().unwrap(), 10)
                .with_property("age".parse().unwrap()),
        );
        // Only create + update-age, not update-city.
        assert_eq!(history.len(), 2);
        assert!(history
            .iter()
            .all(|e| e.changed_properties.iter().all(|p| p.as_str() == "age")));

        // Full snapshot still shows ALL properties at that point.
        assert_eq!(history[0].entity.properties.len(), 2);
    }

    #[test]
    fn cursor_pagination() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let tx1 = exec(
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

        exec(
            &branch,
            TransactionInput::new(meta("update 1")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(26),
                    context: (),
                }],
                meta("birthday 1"),
            )),
        );

        exec(
            &branch,
            TransactionInput::new(meta("update 2")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(27),
                    context: (),
                }],
                meta("birthday 2"),
            )),
        );

        // at_tx = tx1 → should only see the creation.
        let history = query(
            &branch,
            EntityHistoryQuery::new("alice".parse().unwrap(), 10).with_at_tx(tx1.context.tx_id),
        );
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].entity.properties[0].value, serde_json::json!(25));
    }

    #[test]
    fn range_pagination() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec(
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

        let tx2 = exec(
            &branch,
            TransactionInput::new(meta("update 1")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(26),
                    context: (),
                }],
                meta("birthday 1"),
            )),
        );

        let tx3 = exec(
            &branch,
            TransactionInput::new(meta("update 2")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(27),
                    context: (),
                }],
                meta("birthday 2"),
            )),
        );

        // Range: tx2..=tx3 → should see update 1 and update 2, not create.
        let history = query(
            &branch,
            EntityHistoryQuery::new("alice".parse().unwrap(), 10)
                .with_range(tx2.context.tx_id, tx3.context.tx_id),
        );
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].transaction_meta["reasoning"], "update 2");
        assert_eq!(history[1].transaction_meta["reasoning"], "update 1");
    }

    #[test]
    fn branch_ancestry() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec(
            &main,
            TransactionInput::new(meta("create on main")).create_entity(Entity::new(
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

        let checkout_cmd = ExecuteCheckout::new(validator());
        let mut cs = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut cs,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("update on child")).update_entity(Entity::new(
                        "alice".parse().unwrap(),
                        None,
                        vec![PV {
                            property: "age".parse().unwrap(),
                            value: serde_json::json!(30),
                            context: (),
                        }],
                        meta("changed on child"),
                    )),
                ),
            )
            .unwrap();
        cs.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let history = query(
            &child,
            EntityHistoryQuery::new("alice".parse().unwrap(), 10),
        );

        // Should see both: update on child + create on main.
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].transaction_meta["reasoning"], "update on child");
        assert_eq!(history[0].entity.properties[0].value, serde_json::json!(30));
        assert_eq!(history[1].transaction_meta["reasoning"], "create on main");
        assert_eq!(history[1].entity.properties[0].value, serde_json::json!(25));
    }

    #[test]
    fn delete_appears_in_history() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec(
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

        exec(
            &branch,
            TransactionInput::new(meta("delete age")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::Value::Null,
                    context: (),
                }],
                meta("retract age"),
            )),
        );

        let history = query(
            &branch,
            EntityHistoryQuery::new("alice".parse().unwrap(), 10),
        );
        assert_eq!(history.len(), 2);
        // After delete, snapshot should not contain the deleted property.
        assert!(history[0].entity.properties.is_empty());
        // But changed_properties still lists what changed.
        assert_eq!(
            history[0].changed_properties,
            vec!["age".parse::<Slug>().unwrap()]
        );
    }

    #[test]
    fn entity_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        let result = ListEntityHistory::new(test_schema()).execute(
            &branch,
            &mut CommandState::new(branch.storage()),
            EntityHistoryQuery::new("ghost".parse().unwrap(), 10),
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn limit_caps_output() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();

        exec(
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

        exec(
            &branch,
            TransactionInput::new(meta("update 1")).update_entity(Entity::new(
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

        exec(
            &branch,
            TransactionInput::new(meta("update 2")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(27),
                    context: (),
                }],
                meta("another birthday"),
            )),
        );

        let history = query(
            &branch,
            EntityHistoryQuery::new("alice".parse().unwrap(), 2),
        );
        assert_eq!(history.len(), 2);
        // Most recent first.
        assert_eq!(history[0].entity.properties[0].value, serde_json::json!(27));
        assert_eq!(history[1].entity.properties[0].value, serde_json::json!(26));
    }
}
