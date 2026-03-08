use crate::error::ApiError;
use crate::query::Command;
use crate::state::AppState;
use terra_core::assertion::EntityInput;
use terra_core::schema::{AttachInput, EntityTypeInput, PropertyInput};

pub fn dispatch(cmd: Command, state: &AppState) -> Result<serde_yaml::Value, ApiError> {
    let mut inner = state.lock().unwrap();

    match cmd {
        Command::CreateEntityType {
            slug,
            description,
            properties,
            items,
        } => match (slug, items) {
            (Some(slug), None) => {
                let prop_strs: Vec<&str> = properties.iter().map(|s| s.as_str()).collect();
                let input = EntityTypeInput {
                    slug: &slug,
                    description: description.as_deref(),
                    properties: &prop_strs,
                };
                let mut results =
                    inner.registry.create_entity_types_batch(&[input])?;
                let et = results.remove(0);
                Ok(serde_yaml::to_value(&et).unwrap())
            }
            (None, Some(batch_items)) => {
                let inputs: Vec<Vec<&str>> = batch_items
                    .iter()
                    .map(|item| item.properties.iter().map(|s| s.as_str()).collect())
                    .collect();
                let input_refs: Vec<EntityTypeInput<'_>> = batch_items
                    .iter()
                    .zip(inputs.iter())
                    .map(|(item, props)| EntityTypeInput {
                        slug: &item.slug,
                        description: item.description.as_deref(),
                        properties: props,
                    })
                    .collect();
                let results =
                    inner.registry.create_entity_types_batch(&input_refs)?;
                Ok(serde_yaml::to_value(&results).unwrap())
            }
            _ => Err(ApiError::bad_request(
                "parse_error",
                "provide either 'slug' for single creation or 'items' for batch creation, not both",
            )),
        },
        Command::ListEntityTypes => {
            let types = inner.registry.list_entity_types()?;
            Ok(serde_yaml::to_value(&types).unwrap())
        }
        Command::GetEntityType { slug } => {
            let et = inner.registry.get_entity_type(&slug)?;
            let props = inner.registry.list_properties(&slug)?;

            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("id".into()),
                serde_yaml::to_value(&et.id).unwrap(),
            );
            map.insert(
                serde_yaml::Value::String("slug".into()),
                serde_yaml::Value::String(et.slug),
            );
            if let Some(desc) = et.description {
                map.insert(
                    serde_yaml::Value::String("description".into()),
                    serde_yaml::Value::String(desc),
                );
            }
            map.insert(
                serde_yaml::Value::String("created_at".into()),
                serde_yaml::to_value(&et.created_at).unwrap(),
            );
            map.insert(
                serde_yaml::Value::String("properties".into()),
                serde_yaml::to_value(&props).unwrap(),
            );
            Ok(serde_yaml::Value::Mapping(map))
        }
        Command::CreateProperty {
            slug,
            value_type,
            description,
            entity_types,
            items,
        } => match (slug, items) {
            (Some(slug), None) => {
                let value_type = value_type.ok_or_else(|| {
                    ApiError::bad_request("parse_error", "value_type is required")
                })?;
                let et_strs: Vec<&str> = entity_types.iter().map(|s| s.as_str()).collect();
                let input = PropertyInput {
                    slug: &slug,
                    value_type,
                    description: description.as_deref(),
                    entity_types: &et_strs,
                };
                let mut results =
                    inner.registry.create_properties_batch(&[input])?;
                let prop = results.remove(0);
                Ok(serde_yaml::to_value(&prop).unwrap())
            }
            (None, Some(batch_items)) => {
                let inputs: Vec<Vec<&str>> = batch_items
                    .iter()
                    .map(|item| item.entity_types.iter().map(|s| s.as_str()).collect())
                    .collect();
                let input_refs: Vec<PropertyInput<'_>> = batch_items
                    .iter()
                    .zip(inputs.iter())
                    .map(|(item, ets)| PropertyInput {
                        slug: &item.slug,
                        value_type: item.value_type,
                        description: item.description.as_deref(),
                        entity_types: ets,
                    })
                    .collect();
                let results =
                    inner.registry.create_properties_batch(&input_refs)?;
                Ok(serde_yaml::to_value(&results).unwrap())
            }
            _ => Err(ApiError::bad_request(
                "parse_error",
                "provide either 'slug' for single creation or 'items' for batch creation, not both",
            )),
        },
        Command::ListProperties { entity_type: None } => {
            let props = inner.registry.list_all_properties()?;
            Ok(serde_yaml::to_value(&props).unwrap())
        }
        Command::ListProperties {
            entity_type: Some(et),
        } => {
            let props = inner.registry.list_properties(&et)?;
            Ok(serde_yaml::to_value(&props).unwrap())
        }
        Command::AttachProperty {
            entity_type,
            slug,
            items,
        } => match (entity_type.zip(slug), items) {
            (Some((et, slug)), None) => {
                let input = AttachInput {
                    entity_type: &et,
                    property: &slug,
                };
                inner.registry.attach_properties_batch(&[input])?;
                let mut map = serde_yaml::Mapping::new();
                map.insert(
                    serde_yaml::Value::String("status".into()),
                    serde_yaml::Value::String("ok".into()),
                );
                Ok(serde_yaml::Value::Mapping(map))
            }
            (None, Some(batch_items)) => {
                let input_refs: Vec<AttachInput<'_>> = batch_items
                    .iter()
                    .map(|item| AttachInput {
                        entity_type: &item.entity_type,
                        property: &item.slug,
                    })
                    .collect();
                let count =
                    inner.registry.attach_properties_batch(&input_refs)?;
                let mut map = serde_yaml::Mapping::new();
                map.insert(
                    serde_yaml::Value::String("status".into()),
                    serde_yaml::Value::String("ok".into()),
                );
                map.insert(
                    serde_yaml::Value::String("count".into()),
                    serde_yaml::Value::Number(serde_yaml::Number::from(count as u64)),
                );
                Ok(serde_yaml::Value::Mapping(map))
            }
            _ => Err(ApiError::bad_request(
                "parse_error",
                "provide either 'entity_type'+'slug' for single attach or 'items' for batch, not both",
            )),
        },
        Command::CreateEntity {
            entity_name,
            entity_type,
            context,
            items,
        } => match (entity_name, items) {
            (Some(name), None) => {
                let context_json = yaml_context_to_json(context);
                let input = EntityInput {
                    name: &name,
                    entity_type: entity_type.as_deref(),
                    context: context_json,
                };
                let mut results =
                    inner.assertions.create_entities_batch(&[input])?;
                let entry = results.remove(0);
                Ok(serialize_log_entry(&entry))
            }
            (None, Some(batch_items)) => {
                let contexts: Vec<serde_json::Value> = batch_items
                    .iter()
                    .map(|item| yaml_context_to_json(item.context.clone()))
                    .collect();
                let input_refs: Vec<EntityInput<'_>> = batch_items
                    .iter()
                    .zip(contexts.iter())
                    .map(|(item, ctx)| EntityInput {
                        name: &item.entity_name,
                        entity_type: item.entity_type.as_deref(),
                        context: ctx.clone(),
                    })
                    .collect();
                let results =
                    inner.assertions.create_entities_batch(&input_refs)?;
                let entries: Vec<serde_yaml::Value> =
                    results.iter().map(serialize_log_entry).collect();
                Ok(serde_yaml::to_value(&entries).unwrap())
            }
            _ => Err(ApiError::bad_request(
                "parse_error",
                "provide either 'entity_name' for single creation or 'items' for batch, not both",
            )),
        },
        Command::ListLog => {
            let entries = inner.assertions.list_log()?;
            Ok(serde_yaml::to_value(&entries).unwrap())
        }
    }
}

fn yaml_context_to_json(context: Option<serde_yaml::Value>) -> serde_json::Value {
    match context {
        Some(yaml_val) => {
            let json_str = serde_json::to_string(
                &serde_yaml::from_value::<serde_json::Value>(yaml_val)
                    .unwrap_or(serde_json::Value::Null),
            )
            .unwrap_or_default();
            serde_json::from_str(&json_str).unwrap_or(serde_json::Value::Null)
        }
        None => serde_json::json!({}),
    }
}

fn serialize_log_entry(entry: &terra_core::assertion::LogEntry) -> serde_yaml::Value {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("id".into()),
        serde_yaml::to_value(&entry.id).unwrap(),
    );
    map.insert(
        serde_yaml::Value::String("timestamp".into()),
        serde_yaml::Value::String(entry.timestamp.to_rfc3339()),
    );
    map.insert(
        serde_yaml::Value::String("entity_id".into()),
        serde_yaml::to_value(&entry.entity_id).unwrap(),
    );
    if let Some(ref et) = entry.entity_type {
        map.insert(
            serde_yaml::Value::String("entity_type".into()),
            serde_yaml::Value::String(et.clone()),
        );
    }
    map.insert(
        serde_yaml::Value::String("name".into()),
        serde_yaml::Value::String(entry.name.clone()),
    );
    serde_yaml::Value::Mapping(map)
}
