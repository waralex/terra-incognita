//! Entity property queries — latest assertion per property with ancestry walk.

use std::collections::{BTreeSet, HashMap};

use uuid::Uuid;

use crate::config::AssertionStatusesDef;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::assertion::{AssertionEntry, AssertionKey};

/// Get the latest assertion per property for an entity, walking the ancestry chain.
///
/// If `at_tx` is Some, only assertions up to that tx_id are considered.
/// Results are sorted by property slug (stable alphabetical order).
pub fn properties(
    branch: &BranchContext,
    entity: &Slug,
    at_tx: Option<Uuid>,
) -> Result<Vec<AssertionEntry>, DbError> {
    let mut result: HashMap<Slug, AssertionEntry> = HashMap::new();

    let scopes: Vec<_> = match at_tx {
        Some(tx) => branch.scopes_at(tx).collect(),
        None => branch.scopes().collect(),
    };
    for scope in &scopes {
        collect_props(branch, entity, scope.upper_tx, &scope.branch, &mut result)?;
    }

    let mut entries: Vec<AssertionEntry> = result.into_values().collect();
    entries.sort_by(|a, b| a.key.prop.cmp(&b.key.prop));
    Ok(entries)
}

/// Discover props via forward scan, get latest per prop via reverse seek.
fn collect_props(
    branch: &BranchContext,
    entity: &Slug,
    at_tx: Option<Uuid>,
    on_branch: &Slug,
    result: &mut HashMap<Slug, AssertionEntry>,
) -> Result<(), DbError> {
    let mut entity_bound = AssertionKey::bound().with_prefix(|k| {
        k.branch = on_branch.clone();
        k.entity = entity.clone();
    });
    if let Some(tx) = at_tx {
        entity_bound = entity_bound.with_upper(|k| k.tx_id = tx);
    }

    let mut iter = branch.storage().scan::<AssertionEntry>(&entity_bound)?;

    loop {
        let entry = match iter.next() {
            Some(Ok(e)) => e,
            Some(Err(e)) => return Err(e),
            None => break,
        };

        let prop = entry.key.prop.clone();

        if !result.contains_key(&prop) {
            let mut prop_bound = AssertionKey::bound().with_prefix(|k| {
                k.branch = on_branch.clone();
                k.entity = entity.clone();
                k.prop = prop.clone();
            });
            if let Some(tx) = at_tx {
                prop_bound = prop_bound.with_upper(|k| k.tx_id = tx);
            }

            if let Some(latest) = branch.storage().get_latest::<AssertionEntry>(&prop_bound)? {
                if !latest.value.is_deleted() {
                    result.insert(prop.clone(), latest);
                }
            }
        }

        let skip = AssertionKey::bound().with_prefix(|k| {
            k.branch = on_branch.clone();
            k.entity = entity.clone();
            k.prop = prop.clone();
            k.tx_id = Uuid::max();
        });
        iter.seek(&skip);
    }

    Ok(())
}

/// Status-aware snapshot of an entity's properties.
///
/// For each property: the latest terminal assertion forms the baseline, and every
/// non-terminal assertion made *after* it (hypotheses, observations) is layered on
/// top. Anything older than the latest terminal is consolidated away — a terminal
/// assertion resets the picture. Returns a flat list: per property (sorted by slug)
/// the baseline first (if present), then overlays newest-first. Retracted (null)
/// values are omitted; a property whose only baseline is a retraction yields just
/// its later overlays, or nothing.
///
/// Reads are bounded: properties are discovered with a forward scan that seeks past
/// each property's versions, then each property is walked in reverse only down to
/// its latest terminal (per scope). A property with no terminal at all is the only
/// case that reads its full version history — unavoidable, since all its versions
/// are overlays by definition.
pub fn layered_properties(
    branch: &BranchContext,
    entity: &Slug,
    at_tx: Option<Uuid>,
    statuses: &AssertionStatusesDef,
) -> Result<Vec<AssertionEntry>, DbError> {
    let scopes: Vec<_> = match at_tx {
        Some(tx) => branch.scopes_at(tx).collect(),
        None => branch.scopes().collect(),
    };

    let mut props: BTreeSet<Slug> = BTreeSet::new();
    for scope in &scopes {
        discover_props(branch, entity, &scope.branch, &mut props)?;
    }

    let mut result = Vec::new();
    for prop in props {
        // Per scope, walk newest-first down to that scope's latest terminal,
        // collecting the overlays above it. The global baseline is the newest
        // terminal across scopes; overlays older than it were consolidated away.
        let mut overlays = Vec::new();
        let mut baseline: Option<AssertionEntry> = None;
        for scope in &scopes {
            let mut bound = AssertionKey::bound().with_prefix(|k| {
                k.branch = scope.branch.clone();
                k.entity = entity.clone();
                k.prop = prop.clone();
            });
            if let Some(upper) = scope.upper_tx {
                bound = bound.with_upper(|k| k.tx_id = upper);
            }

            for entry in branch.storage().scan_rev::<AssertionEntry>(&bound)? {
                let entry = entry?;
                if statuses.is_terminal(entry.value.status.as_deref()) {
                    if baseline
                        .as_ref()
                        .is_none_or(|b| entry.key.tx_id > b.key.tx_id)
                    {
                        baseline = Some(entry);
                    }
                    break;
                }
                overlays.push(entry);
            }
        }

        let baseline_tx = baseline.as_ref().map(|b| b.key.tx_id);
        let mut kept: Vec<AssertionEntry> = overlays
            .into_iter()
            .filter(|o| baseline_tx.is_none_or(|bt| o.key.tx_id > bt))
            .filter(|o| !o.value.is_deleted())
            .collect();
        kept.sort_by(|a, b| b.key.tx_id.cmp(&a.key.tx_id));

        if let Some(base) = baseline {
            if !base.value.is_deleted() {
                result.push(base);
            }
        }
        result.extend(kept);
    }

    Ok(result)
}

/// Discover the distinct property slugs for an entity on one branch.
///
/// Forward-scans, seeking past every version of each property — reads roughly one
/// entry per property, not the full version history.
fn discover_props(
    branch: &BranchContext,
    entity: &Slug,
    on_branch: &Slug,
    props: &mut BTreeSet<Slug>,
) -> Result<(), DbError> {
    let entity_bound = AssertionKey::bound().with_prefix(|k| {
        k.branch = on_branch.clone();
        k.entity = entity.clone();
    });

    let mut iter = branch.storage().scan::<AssertionEntry>(&entity_bound)?;
    loop {
        let entry = match iter.next() {
            Some(Ok(e)) => e,
            Some(Err(e)) => return Err(e),
            None => break,
        };

        let prop = entry.key.prop.clone();
        props.insert(prop.clone());

        let skip = AssertionKey::bound().with_prefix(|k| {
            k.branch = on_branch.clone();
            k.entity = entity.clone();
            k.prop = prop.clone();
            k.tx_id = Uuid::max();
        });
        iter.seek(&skip);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::executor::checkout::ExecuteCheckout;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::checkout::CheckoutInput;
    use crate::command::input::transaction::TransactionInput;
    use crate::command::Command;
    use crate::command::CommandState;
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::{Entity, PropertyValue as PV};
    use crate::domain::transaction::Transaction;
    use crate::domain::tx_meta::TxMeta;
    use crate::domain::validator::DomainValidator;
    use crate::store::storage::Storage;
    use indoc::indoc;
    use std::sync::Arc;

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

    fn meta(r: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("reasoning".into(), serde_json::json!(r));
        m
    }

    fn exec(branch: &BranchContext, input: TransactionInput) -> Transaction<TxMeta> {
        let cmd = ExecuteTransaction::new(validator());
        let mut state = CommandState::new(branch.storage());
        let result = cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
        result
    }

    #[test]
    fn returns_latest_per_property() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

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

        let props = properties(&branch, &"alice".parse().unwrap(), None).unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].key.prop.as_str(), "age");
        assert_eq!(props[0].value.value, serde_json::json!(26));
        assert_eq!(props[1].key.prop.as_str(), "city");
        assert_eq!(props[1].value.value, serde_json::json!("London"));
    }

    #[test]
    fn deleted_property_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

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
            TransactionInput::new(meta("delete age")).update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    property: "age".parse().unwrap(),
                    value: serde_json::Value::Null,
                    context: (),
                }],
                meta("age retracted"),
            )),
        );

        let props = properties(&branch, &"alice".parse().unwrap(), None).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].key.prop.as_str(), "city");
        assert_eq!(props[0].value.value, serde_json::json!("London"));
    }

    #[test]
    fn at_tx_filters() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

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

        let props =
            properties(&branch, &"alice".parse().unwrap(), Some(tx1.context.tx_id)).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value.value, serde_json::json!(25));
    }

    #[test]
    fn empty_for_unknown_entity() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let props = properties(&branch, &"ghost".parse().unwrap(), None).unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn inherits_from_parent_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec(
            &main,
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

        let checkout_cmd = ExecuteCheckout::new(validator());
        let mut state = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut state,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("update age")).update_entity(Entity::new(
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
        state.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let props = properties(&child, &"alice".parse().unwrap(), None).unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].key.prop.as_str(), "age");
        assert_eq!(props[0].value.value, serde_json::json!(30));
        assert_eq!(props[0].key.branch.as_str(), "child");
        assert_eq!(props[1].key.prop.as_str(), "city");
        assert_eq!(props[1].value.value, serde_json::json!("London"));
        assert_eq!(props[1].key.branch.as_str(), "main");
    }

    // --- Status layering ---

    fn status_schema() -> Arc<DataSchema> {
        Arc::new(
            DataSchema::from_yaml(indoc! {"
            transaction_meta:
              reasoning: { type: text, required: true }
            entity_change_meta:
              reasoning: { type: text, required: true }
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

    fn exec_s(branch: &BranchContext, schema: Arc<DataSchema>, input: TransactionInput) {
        let cmd = ExecuteTransaction::new(DomainValidator::new(schema));
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
    }

    /// Build an entity asserting a single `capital` property with a status.
    fn capital(value: serde_json::Value, reasoning: &str, status: &str) -> Entity {
        Entity::new(
            "alice".parse().unwrap(),
            Some(serde_json::json!("a place")),
            vec![PV {
                property: "capital".parse().unwrap(),
                value,
                context: (),
            }],
            meta(reasoning),
        )
        .with_status(Some(status.into()))
    }

    #[test]
    fn layering_latest_fact_plus_later_overlays() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let schema = status_schema();

        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t1")).create_entity(capital(
                serde_json::json!("Lyon"),
                "old fact",
                "fact",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t2")).update_entity(capital(
                serde_json::json!("Paris?"),
                "guess",
                "hypothesis",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t3")).update_entity(capital(
                serde_json::json!("doc says Paris"),
                "obs",
                "observation",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t4")).update_entity(capital(
                serde_json::json!("Paris"),
                "settled",
                "fact",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t5")).update_entity(capital(
                serde_json::json!("might move"),
                "new guess",
                "hypothesis",
            )),
        );

        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();

        // Baseline fact@t4 + the single hypothesis thrown after it (t5).
        // Everything before t4 is consolidated away.
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].value.value, serde_json::json!("Paris"));
        assert_eq!(props[0].value.status.as_deref(), Some("fact"));
        assert_eq!(props[1].value.value, serde_json::json!("might move"));
        assert_eq!(props[1].value.status.as_deref(), Some("hypothesis"));
    }

    #[test]
    fn layering_no_fact_returns_all_overlays() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let schema = status_schema();

        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t1")).create_entity(capital(
                serde_json::json!("Paris?"),
                "guess",
                "hypothesis",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t2")).update_entity(capital(
                serde_json::json!("seen Paris"),
                "obs",
                "observation",
            )),
        );

        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();
        assert_eq!(props.len(), 2);
        assert!(props.iter().all(|p| p.value.status.as_deref() != Some("fact")));
    }

    #[test]
    fn layering_drops_overlays_older_than_fact() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let schema = status_schema();

        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t1")).create_entity(capital(
                serde_json::json!("Paris?"),
                "guess",
                "hypothesis",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t2")).update_entity(capital(
                serde_json::json!("Paris"),
                "settled",
                "fact",
            )),
        );

        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();
        // The earlier hypothesis is consolidated by the fact — only the fact remains.
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value.value, serde_json::json!("Paris"));
        assert_eq!(props[0].value.status.as_deref(), Some("fact"));
    }

    #[test]
    fn layering_retraction_suppresses_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let schema = status_schema();

        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t1")).create_entity(capital(
                serde_json::json!("Paris"),
                "settled",
                "fact",
            )),
        );
        // Retract via a null-valued fact.
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t2")).update_entity(capital(
                serde_json::Value::Null,
                "retract",
                "fact",
            )),
        );

        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();
        assert!(props.is_empty());

        // A later hypothesis re-opens the property on top of the retraction.
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t3")).update_entity(capital(
                serde_json::json!("maybe Lyon"),
                "reopen",
                "hypothesis",
            )),
        );
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value.status.as_deref(), Some("hypothesis"));
    }

    #[test]
    fn layering_across_ancestry() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        let schema = status_schema();

        exec_s(
            &main,
            schema.clone(),
            TransactionInput::new(meta("t1")).create_entity(capital(
                serde_json::json!("Paris"),
                "settled",
                "fact",
            )),
        );

        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(schema.clone()));
        let mut state = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut state,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("guess on child")).update_entity(capital(
                        serde_json::json!("maybe Lyon"),
                        "child guess",
                        "hypothesis",
                    )),
                ),
            )
            .unwrap();
        state.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&child, &"alice".parse().unwrap(), None, statuses).unwrap();
        // Baseline fact inherited from main + hypothesis thrown on the child.
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].value.value, serde_json::json!("Paris"));
        assert_eq!(props[0].value.status.as_deref(), Some("fact"));
        assert_eq!(props[0].key.branch.as_str(), "main");
        assert_eq!(props[1].value.status.as_deref(), Some("hypothesis"));
        assert_eq!(props[1].key.branch.as_str(), "child");
    }

    #[test]
    fn sorted_by_slug() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(
            &branch,
            TransactionInput::new(meta("create")).create_entity(Entity::new(
                "server".parse().unwrap(),
                Some(serde_json::json!("A server")),
                vec![
                    PV {
                        property: "zone".parse().unwrap(),
                        value: serde_json::json!("us-east"),
                        context: (),
                    },
                    PV {
                        property: "cpu".parse().unwrap(),
                        value: serde_json::json!(8),
                        context: (),
                    },
                    PV {
                        property: "memory".parse().unwrap(),
                        value: serde_json::json!("32gb"),
                        context: (),
                    },
                ],
                meta("initial"),
            )),
        );

        let props = properties(&branch, &"server".parse().unwrap(), None).unwrap();
        let slugs: Vec<&str> = props.iter().map(|p| p.key.prop.as_str()).collect();
        assert_eq!(slugs, vec!["cpu", "memory", "zone"]);
    }
}
