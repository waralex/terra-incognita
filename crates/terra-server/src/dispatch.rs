use crate::error::ApiError;
use crate::query::{QueryDto, ResponseShape};
use crate::state::AppState;
use terra_core::assertion::LogEntry;
use terra_core::command::CommandResult;

/// Parses a DTO into a domain command, executes it, and serializes the result.
pub fn dispatch(dto: QueryDto, state: &AppState) -> Result<serde_yaml::Value, ApiError> {
    let (cmd, shape) = dto.into_command()?;
    let mut inner = state.lock().unwrap();
    let crate::state::Inner {
        ref mut registry,
        ref assertions,
    } = *inner;
    let result = terra_core::command::execute(cmd, registry, assertions)?;
    Ok(serialize_result(result, shape))
}

fn serialize_result(result: CommandResult, shape: ResponseShape) -> serde_yaml::Value {
    match result {
        CommandResult::EntityTypes(types) => match shape {
            ResponseShape::Single => serde_yaml::to_value(&types[0]).unwrap(),
            ResponseShape::Batch => serde_yaml::to_value(&types).unwrap(),
        },
        CommandResult::Properties(props) => match shape {
            ResponseShape::Single => serde_yaml::to_value(&props[0]).unwrap(),
            ResponseShape::Batch => serde_yaml::to_value(&props).unwrap(),
        },
        CommandResult::Attached { count } => {
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("status".into()),
                serde_yaml::Value::String("ok".into()),
            );
            if let ResponseShape::Batch = shape {
                map.insert(
                    serde_yaml::Value::String("count".into()),
                    serde_yaml::Value::Number(serde_yaml::Number::from(count as u64)),
                );
            }
            serde_yaml::Value::Mapping(map)
        }
        CommandResult::Entities(entries) => match shape {
            ResponseShape::Single => serialize_log_entry(&entries[0]),
            ResponseShape::Batch => {
                let items: Vec<serde_yaml::Value> =
                    entries.iter().map(serialize_log_entry).collect();
                serde_yaml::to_value(&items).unwrap()
            }
        },
        CommandResult::EntityTypeDetail {
            entity_type,
            properties,
        } => {
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("id".into()),
                serde_yaml::to_value(&entity_type.id).unwrap(),
            );
            map.insert(
                serde_yaml::Value::String("slug".into()),
                serde_yaml::Value::String(entity_type.slug),
            );
            if let Some(desc) = entity_type.description {
                map.insert(
                    serde_yaml::Value::String("description".into()),
                    serde_yaml::Value::String(desc),
                );
            }
            map.insert(
                serde_yaml::Value::String("created_at".into()),
                serde_yaml::to_value(&entity_type.created_at).unwrap(),
            );
            map.insert(
                serde_yaml::Value::String("properties".into()),
                serde_yaml::to_value(&properties).unwrap(),
            );
            serde_yaml::Value::Mapping(map)
        }
        CommandResult::LogEntries(entries) => {
            let items: Vec<serde_yaml::Value> =
                entries.iter().map(serialize_log_entry).collect();
            serde_yaml::to_value(&items).unwrap()
        }
    }
}

fn serialize_log_entry(entry: &LogEntry) -> serde_yaml::Value {
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
    map.insert(
        serde_yaml::Value::String("properties".into()),
        serde_yaml::to_value(&entry.properties).unwrap(),
    );
    map.insert(
        serde_yaml::Value::String("reasoning".into()),
        serde_yaml::to_value(&entry.reasoning).unwrap(),
    );
    serde_yaml::Value::Mapping(map)
}
