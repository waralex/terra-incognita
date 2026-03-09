use uuid::Uuid;

use crate::assertion::AssertionStore;
use crate::schema::{AttachInput, EntityTypeInput, PropertyInput, SchemaRegistry};

use super::{Command, CommandError, CommandResult};

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
        Command::CreateEntities(items) => {
            let batch: Vec<(Uuid, serde_json::Value)> = items
                .iter()
                .map(|item| {
                    let entity_id = Uuid::now_v7();
                    let body = serde_json::json!({
                        "entity_name": item.entity_name,
                        "entity_type": item.entity_type,
                        "context": item.context,
                    });
                    (entity_id, body)
                })
                .collect();
            let results = store.refinements().append_batch(&batch)?;
            Ok(CommandResult::Entities(results))
        }
        Command::ListLog => {
            let entries = store.refinements().list()?;
            Ok(CommandResult::LogEntries(entries))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CreateEntityType, CreateProperty, AttachProperty, CreateEntity};
    use crate::schema::ValueType;

    fn setup() -> (SchemaRegistry, AssertionStore, tempfile::TempDir) {
        let registry = SchemaRegistry::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (registry, store, dir)
    }

    #[test]
    fn create_and_list_entity_types() {
        let (mut reg, store, _dir) = setup();

        let result = execute(
            Command::CreateEntityTypes(vec![
                CreateEntityType {
                    slug: "unit".into(),
                    description: Some("Research project".into()),
                    properties: vec![],
                },
            ]),
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
    fn create_entities_and_list_log() {
        let (mut reg, store, _dir) = setup();

        let result = execute(
            Command::CreateEntities(vec![
                CreateEntity {
                    entity_name: "alpha".into(),
                    entity_type: Some("unit".into()),
                    context: serde_json::json!({}),
                },
                CreateEntity {
                    entity_name: "bravo".into(),
                    entity_type: None,
                    context: serde_json::json!({"source": "test"}),
                },
            ]),
            &mut reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::Entities(entries) => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].body["entity_name"], "alpha");
                assert_eq!(entries[1].body["entity_name"], "bravo");
            }
            _ => panic!("unexpected result"),
        }

        let result = execute(Command::ListLog, &mut reg, &store).unwrap();
        match result {
            CommandResult::LogEntries(entries) => assert_eq!(entries.len(), 2),
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
