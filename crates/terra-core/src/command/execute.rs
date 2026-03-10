use crate::assertion::AssertionStore;
use crate::schema::{AttachInput, EntityTypeInput, PropertyInput, SchemaRegistry};

use super::{Command, CommandError, CommandResult};
use super::assert_entity;
use super::query_entity;
use super::session;

/// Executes a domain command against the schema registry and assertion store.
pub fn execute(
    cmd: Command,
    registry: &mut SchemaRegistry,
    store: &AssertionStore,
) -> Result<CommandResult, CommandError> {
    match cmd {
        Command::CreateEntityTypes(items) => {
            let prop_strs: Vec<Vec<&str>> = items
                .iter()
                .map(|item| item.properties.iter().map(|s| s.as_str()).collect())
                .collect();
            let inputs: Vec<EntityTypeInput<'_>> = items
                .iter()
                .zip(prop_strs.iter())
                .map(|(item, props)| EntityTypeInput {
                    slug: &item.slug,
                    description: item.description.as_deref(),
                    properties: props,
                })
                .collect();
            let results = registry.create_entity_types_batch(&inputs)?;
            Ok(CommandResult::EntityTypes(results))
        }
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
        Command::CreateProperties(items) => {
            let et_strs: Vec<Vec<&str>> = items
                .iter()
                .map(|item| item.entity_types.iter().map(|s| s.as_str()).collect())
                .collect();
            let inputs: Vec<PropertyInput<'_>> = items
                .iter()
                .zip(et_strs.iter())
                .map(|(item, ets)| PropertyInput {
                    slug: &item.slug,
                    value_type: item.value_type,
                    description: item.description.as_deref(),
                    entity_types: ets,
                })
                .collect();
            let results = registry.create_properties_batch(&inputs)?;
            Ok(CommandResult::Properties(results))
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
        Command::AttachProperties(items) => {
            let inputs: Vec<AttachInput<'_>> = items
                .iter()
                .map(|item| AttachInput {
                    entity_type: &item.entity_type,
                    property: &item.property,
                })
                .collect();
            let count = registry.attach_properties_batch(&inputs)?;
            Ok(CommandResult::Attached { count })
        }
        Command::CreateEntity(input) => {
            let result = assert_entity::create_entity(input, registry, store)?;
            Ok(CommandResult::Asserted {
                transaction: result.transaction,
                facts: result.facts,
                hypotheses: result.hypotheses,
            })
        }
        Command::AssertEntity(input) => {
            let result = assert_entity::assert_entity(input, registry, store)?;
            Ok(CommandResult::Asserted {
                transaction: result.transaction,
                facts: result.facts,
                hypotheses: result.hypotheses,
            })
        }
        Command::Transaction(input) => {
            let result = assert_entity::execute_transaction(input, registry, store)?;
            Ok(CommandResult::TransactionResult {
                transaction: result.transaction,
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
        Command::CreateSession(input) => {
            let detail = session::create_session(input, registry, store)?;
            Ok(CommandResult::Session(detail))
        }
        Command::GetSession { slug } => {
            let detail = session::get_session(&slug, registry, store)?;
            Ok(CommandResult::Session(detail))
        }
        Command::ListSessions => {
            let list = session::list_sessions(store)?;
            Ok(CommandResult::SessionList(list))
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
        AssertEntityInput, AssertionItem, CreateEntityType, CreateProperty, AttachProperty,
    };
    use crate::assertion::{PropertyValue, RangeValue, SetValue};
    use crate::schema::ValueType;
    use std::collections::HashMap;

    fn setup() -> (SchemaRegistry, AssertionStore, tempfile::TempDir) {
        let registry = SchemaRegistry::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (registry, store, dir)
    }

    fn setup_schema(reg: &mut SchemaRegistry, store: &AssertionStore) {
        execute(
            Command::CreateProperties(vec![
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
            ]),
            reg,
            store,
        )
        .unwrap();

        execute(
            Command::CreateEntityTypes(vec![CreateEntityType {
                slug: "track".into(),
                description: None,
                properties: vec!["bpm".into(), "certification".into()],
            }]),
            reg,
            store,
        )
        .unwrap();
    }

    #[test]
    fn create_and_list_entity_types() {
        let (mut reg, store, _dir) = setup();

        let result = execute(
            Command::CreateEntityTypes(vec![CreateEntityType {
                slug: "unit".into(),
                description: Some("Research project".into()),
                properties: vec![],
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::EntityTypes(types) => {
                assert_eq!(types.len(), 1);
                assert_eq!(types[0].slug, "unit");
            }
            _ => panic!("unexpected result"),
        }

        let result = execute(Command::ListEntityTypes, &mut reg, &store).unwrap();
        match result {
            CommandResult::EntityTypes(types) => assert_eq!(types.len(), 1),
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn get_entity_type_detail() {
        let (mut reg, store, _dir) = setup();

        execute(
            Command::CreateProperties(vec![CreateProperty {
                slug: "name".into(),
                value_type: ValueType::Struct,
                description: None,
                entity_types: vec![],
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        execute(
            Command::CreateEntityTypes(vec![CreateEntityType {
                slug: "unit".into(),
                description: None,
                properties: vec!["name".into()],
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        let result = execute(
            Command::GetEntityType { slug: "unit".into() },
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::EntityTypeDetail {
                entity_type,
                properties,
            } => {
                assert_eq!(entity_type.slug, "unit");
                assert_eq!(properties.len(), 1);
                assert_eq!(properties[0].slug, "name");
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn create_properties_with_entity_type_attachment() {
        let (mut reg, store, _dir) = setup();

        execute(
            Command::CreateEntityTypes(vec![CreateEntityType {
                slug: "unit".into(),
                description: None,
                properties: vec![],
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        let result = execute(
            Command::CreateProperties(vec![CreateProperty {
                slug: "name".into(),
                value_type: ValueType::Struct,
                description: None,
                entity_types: vec!["unit".into()],
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::Properties(props) => {
                assert_eq!(props.len(), 1);
                assert_eq!(props[0].slug, "name");
            }
            _ => panic!("unexpected result"),
        }

        let result = execute(
            Command::ListProperties {
                entity_type: Some("unit".into()),
            },
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::Properties(props) => assert_eq!(props.len(), 1),
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn attach_properties() {
        let (mut reg, store, _dir) = setup();

        execute(
            Command::CreateEntityTypes(vec![CreateEntityType {
                slug: "unit".into(),
                description: None,
                properties: vec![],
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        execute(
            Command::CreateProperties(vec![CreateProperty {
                slug: "code".into(),
                value_type: ValueType::Range,
                description: None,
                entity_types: vec![],
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        let result = execute(
            Command::AttachProperties(vec![AttachProperty {
                entity_type: "unit".into(),
                property: "code".into(),
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::Attached { count } => assert_eq!(count, 1),
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn create_entity_via_execute() {
        let (mut reg, store, _dir) = setup();
        setup_schema(&mut reg, &store);

        let result = execute(
            Command::CreateEntity(AssertEntityInput {
                entity: "song-1".into(),
                description: Some("A test song".into()),
                reasoning: serde_json::json!("initial setup"),
                facts: vec![AssertionItem {
                    entity_type: "track".into(),
                    properties: HashMap::from([(
                        "bpm".into(),
                        PropertyValue::Range(RangeValue::Eq(serde_json::json!(128))),
                    )]),
                    reasoning: serde_json::json!("detected"),
                }],
                hypotheses: vec![],
            }),
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::Asserted {
                transaction,
                facts,
                hypotheses,
            } => {
                assert_eq!(facts.len(), 1);
                assert!(hypotheses.is_empty());
                assert!(facts[0].tx_id.is_some());
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn assert_entity_via_execute() {
        let (mut reg, store, _dir) = setup();
        setup_schema(&mut reg, &store);

        // Create entity first
        execute(
            Command::CreateEntity(AssertEntityInput {
                entity: "song-2".into(),
                description: None,
                reasoning: serde_json::json!(null),
                facts: vec![],
                hypotheses: vec![],
            }),
            &mut reg,
            &store,
        )
        .unwrap();

        // Assert on it
        let result = execute(
            Command::AssertEntity(AssertEntityInput {
                entity: "song-2".into(),
                description: None,
                reasoning: serde_json::json!("follow-up"),
                facts: vec![],
                hypotheses: vec![AssertionItem {
                    entity_type: "track".into(),
                    properties: HashMap::from([(
                        "bpm".into(),
                        PropertyValue::Range(RangeValue::Eq(serde_json::json!(120))),
                    )]),
                    reasoning: serde_json::json!("estimate"),
                }],
            }),
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::Asserted { hypotheses, .. } => {
                assert_eq!(hypotheses.len(), 1);
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn batch_entity_types() {
        let (mut reg, store, _dir) = setup();

        let result = execute(
            Command::CreateEntityTypes(vec![
                CreateEntityType {
                    slug: "alpha".into(),
                    description: None,
                    properties: vec![],
                },
                CreateEntityType {
                    slug: "bravo".into(),
                    description: Some("Second".into()),
                    properties: vec![],
                },
            ]),
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::EntityTypes(types) => {
                assert_eq!(types.len(), 2);
                assert_eq!(types[0].slug, "alpha");
                assert_eq!(types[1].slug, "bravo");
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn error_propagation() {
        let (mut reg, store, _dir) = setup();

        let err = execute(
            Command::GetEntityType {
                slug: "nonexistent".into(),
            },
            &mut reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, CommandError::Schema(_)));
    }
}
