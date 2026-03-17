//! Entity property queries — latest assertion per property with ancestry walk.

use std::collections::HashMap;

use uuid::Uuid;

use crate::io::DbError;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
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
    let entity_bound = AssertionKey::bound()
        .with_prefix(|k| {
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

        if !result.contains_key(&prop) {
            let mut prop_bound = AssertionKey::bound()
                .with_prefix(|k| {
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

        let skip = AssertionKey::bound()
            .with_prefix(|k| {
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
    use crate::command::Command;
    use crate::command::CommandState;
    use crate::command::executor::checkout::ExecuteCheckout;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::checkout::CheckoutInput;
    use crate::command::input::transaction::TransactionInput;
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
        "}).unwrap())
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

        exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                    PV { property: "city".parse().unwrap(), value: serde_json::json!("London"), context: () },
                ],
                meta("initial"),
            )));

        exec(&branch, TransactionInput::new(meta("update"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(26), context: () },
                ],
                meta("birthday"),
            )));

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

        exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                    PV { property: "city".parse().unwrap(), value: serde_json::json!("London"), context: () },
                ],
                meta("initial"),
            )));

        exec(&branch, TransactionInput::new(meta("delete age"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::Value::Null, context: () },
                ],
                meta("age retracted"),
            )));

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

        let tx1 = exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                ],
                meta("initial"),
            )));

        exec(&branch, TransactionInput::new(meta("update"))
            .update_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(26), context: () },
                ],
                meta("birthday"),
            )));

        let props = properties(&branch, &"alice".parse().unwrap(), Some(tx1.context.tx_id)).unwrap();
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

        exec(&main, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV { property: "age".parse().unwrap(), value: serde_json::json!(25), context: () },
                    PV { property: "city".parse().unwrap(), value: serde_json::json!("London"), context: () },
                ],
                meta("initial"),
            )));

        let checkout_cmd = ExecuteCheckout::new(validator());
        let mut state = CommandState::new(&storage);
        checkout_cmd.execute(&main, &mut state, CheckoutInput::new(
            "child".parse().unwrap(),
            meta("explore"),
            None,
            TransactionInput::new(meta("update age"))
                .update_entity(Entity::new(
                    "alice".parse().unwrap(),
                    None,
                    vec![
                        PV { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () },
                    ],
                    meta("changed on child"),
                )),
        )).unwrap();
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

    #[test]
    fn sorted_by_slug() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(&branch, TransactionInput::new(meta("create"))
            .create_entity(Entity::new(
                "server".parse().unwrap(),
                Some(serde_json::json!("A server")),
                vec![
                    PV { property: "zone".parse().unwrap(), value: serde_json::json!("us-east"), context: () },
                    PV { property: "cpu".parse().unwrap(), value: serde_json::json!(8), context: () },
                    PV { property: "memory".parse().unwrap(), value: serde_json::json!("32gb"), context: () },
                ],
                meta("initial"),
            )));

        let props = properties(&branch, &"server".parse().unwrap(), None).unwrap();
        let slugs: Vec<&str> = props.iter().map(|p| p.key.prop.as_str()).collect();
        assert_eq!(slugs, vec!["cpu", "memory", "zone"]);
    }
}
