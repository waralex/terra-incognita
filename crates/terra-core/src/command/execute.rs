use crate::assertion::AssertionStore;
use crate::schema::BranchSchemaRegistry;

use super::{Command, CommandError, CommandResult};
use super::assert_entity;
use super::query_entity;
use super::branch;

/// Executes a domain command against the schema registry and assertion store.
pub fn execute(
    cmd: Command,
    registry: &BranchSchemaRegistry,
    store: &AssertionStore,
) -> Result<CommandResult, CommandError> {
    match cmd {
        Command::ListEntityTypes => {
            let types = registry.list_entity_types()?;
            Ok(CommandResult::EntityTypes(types))
        }
        Command::GetEntityType { slug } => {
            let entity_type = registry.get_entity_type(&slug)?;
            let properties = registry.list_properties(&slug)?;
            Ok(CommandResult::EntityTypeDetail {
                entity_type,
                properties,
            })
        }
        Command::ListProperties {
            entity_type: None,
        } => {
            let props = registry.list_all_properties()?;
            Ok(CommandResult::Properties(props))
        }
        Command::ListProperties {
            entity_type: Some(et),
        } => {
            let props = registry.list_properties(&et)?;
            Ok(CommandResult::Properties(props))
        }
        Command::Transaction(input) => {
            let result = assert_entity::execute_transaction(input, registry, store)?;
            Ok(CommandResult::TransactionResult {
                transaction: result.transaction,
                entity_types: result.entity_types,
                properties: result.properties,
                attached_count: result.attached_count,
                introduced: result.introduced,
                asserted: result.asserted,
            })
        }
        Command::ListEntities => {
            let entities = store.entities().list_active()?;
            Ok(CommandResult::EntityList(entities))
        }
        Command::GetEntity {
            entity,
            entity_type,
        } => {
            let projection = query_entity::project_entity(&entity, &entity_type, registry, store)?;
            Ok(CommandResult::EntityDetail(projection))
        }
        Command::CreateBranch(input) => {
            let detail = branch::create_branch(input, store)?;
            Ok(CommandResult::Branch(detail))
        }
        Command::GetBranch { slug } => {
            let detail = branch::get_branch(&slug, store)?;
            Ok(CommandResult::Branch(detail))
        }
        Command::ListBranches => {
            let list = branch::list_branches(store)?;
            Ok(CommandResult::BranchList(list))
        }
        Command::ListLog => {
            let entries = store.facts().list()?;
            Ok(CommandResult::LogEntries(entries))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{
        AssertionItem, CreateEntityType, CreateProperty, HideUnhideInput,
        IntroduceItem, TransactionInput,
    };
    use crate::assertion::{PropertyValue, RangeValue, MAIN_BRANCH};
    use crate::schema::ValueType;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn setup() -> (BranchSchemaRegistry, AssertionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        let registry = store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
        (registry, store, dir)
    }

    fn setup_schema(reg: &BranchSchemaRegistry, store: &AssertionStore) {
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![],
                properties: vec![
                    CreateProperty {
                        slug: "bpm".into(),
                        value_type: ValueType::Range,
                        description: None,
                        entity_types: vec![],
                    },
                    CreateProperty {
                        slug: "certification".into(),
                        value_type: ValueType::Set,
                        description: None,
                        entity_types: vec![],
                    },
                ],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            reg,
            store,
        )
        .unwrap();

        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![CreateEntityType {
                    slug: "track".into(),
                    description: None,
                    properties: vec!["bpm".into(), "certification".into()],
                }],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            reg,
            store,
        )
        .unwrap();
    }

    #[test]
    fn create_and_list_entity_types() {
        let (reg, store, _dir) = setup();

        let result = execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![CreateEntityType {
                    slug: "unit".into(),
                    description: Some("Research project".into()),
                    properties: vec![],
                }],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::TransactionResult { entity_types, .. } => {
                assert_eq!(entity_types.len(), 1);
                assert_eq!(entity_types[0].slug, "unit");
            }
            _ => panic!("unexpected result"),
        }

        let result = execute(Command::ListEntityTypes, &reg, &store).unwrap();
        match result {
            CommandResult::EntityTypes(types) => assert_eq!(types.len(), 1),
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn create_entity_via_transaction() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        let result = execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("initial setup"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![IntroduceItem {
                    entity: "song-1".into(),
                    description: Some("A test song".into()),
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(serde_json::json!(128))),
                        )]),
                        reasoning: serde_json::json!("detected"),
                    }],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::TransactionResult {
                introduced,
                ..
            } => {
                assert_eq!(introduced.len(), 1);
                assert_eq!(introduced[0].facts.len(), 1);
                assert!(introduced[0].hypotheses.is_empty());
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn list_entities_after_create() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![IntroduceItem {
                    entity: "alpha".into(),
                    description: None,
                    facts: vec![],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        let result = execute(Command::ListEntities, &reg, &store).unwrap();
        match result {
            CommandResult::EntityList(entities) => {
                assert_eq!(entities.len(), 1);
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn error_propagation() {
        let (reg, store, _dir) = setup();

        let err = execute(
            Command::GetEntityType {
                slug: "nonexistent".into(),
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, CommandError::Schema(_)));
    }
}
