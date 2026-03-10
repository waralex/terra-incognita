use crate::error::ApiError;
use crate::query::{QueryDto, ResponseShape};
use crate::state::AppState;
use terra_core::assertion::LogEntry;
use terra_core::command::{CommandResult, TransactionEntityResult};

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
        CommandResult::Asserted {
            transaction,
            facts,
            hypotheses,
        } => {
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("tx_id".into()),
                serde_yaml::to_value(&transaction.id).unwrap(),
            );
            if !facts.is_empty() {
                let items: Vec<serde_yaml::Value> =
                    facts.iter().map(serialize_log_entry).collect();
                map.insert(
                    serde_yaml::Value::String("facts".into()),
                    serde_yaml::to_value(&items).unwrap(),
                );
            }
            if !hypotheses.is_empty() {
                let items: Vec<serde_yaml::Value> =
                    hypotheses.iter().map(serialize_log_entry).collect();
                map.insert(
                    serde_yaml::Value::String("hypotheses".into()),
                    serde_yaml::to_value(&items).unwrap(),
                );
            }
            serde_yaml::Value::Mapping(map)
        }
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
        CommandResult::TransactionResult {
            transaction,
            introduced,
            asserted,
        } => {
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("tx_id".into()),
                serde_yaml::to_value(&transaction.id).unwrap(),
            );
            if !introduced.is_empty() {
                let items: Vec<serde_yaml::Value> = introduced
                    .iter()
                    .map(serialize_entity_result)
                    .collect();
                map.insert(
                    serde_yaml::Value::String("introduce".into()),
                    serde_yaml::to_value(&items).unwrap(),
                );
            }
            if !asserted.is_empty() {
                let items: Vec<serde_yaml::Value> = asserted
                    .iter()
                    .map(serialize_entity_result)
                    .collect();
                map.insert(
                    serde_yaml::Value::String("asserts".into()),
                    serde_yaml::to_value(&items).unwrap(),
                );
            }
            serde_yaml::Value::Mapping(map)
        }
        CommandResult::Session(detail) => {
            let mut map = serde_yaml::Mapping::new();
            map.insert(
                serde_yaml::Value::String("id".into()),
                serde_yaml::to_value(&detail.id).unwrap(),
            );
            map.insert(
                serde_yaml::Value::String("slug".into()),
                serde_yaml::Value::String(detail.slug),
            );
            if let Some(desc) = detail.description {
                map.insert(
                    serde_yaml::Value::String("description".into()),
                    serde_yaml::Value::String(desc),
                );
            }
            let et_list: Vec<serde_yaml::Value> = detail
                .entity_types
                .iter()
                .map(|et| {
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("slug".into()),
                        serde_yaml::Value::String(et.slug.clone()),
                    );
                    serde_yaml::Value::Mapping(m)
                })
                .collect();
            map.insert(
                serde_yaml::Value::String("entity_types".into()),
                serde_yaml::to_value(&et_list).unwrap(),
            );
            let seed_list: Vec<serde_yaml::Value> = detail
                .seed_entities
                .iter()
                .map(|e| {
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("slug".into()),
                        serde_yaml::Value::String(e.slug.clone()),
                    );
                    m.insert(
                        serde_yaml::Value::String("id".into()),
                        serde_yaml::to_value(&e.id).unwrap(),
                    );
                    serde_yaml::Value::Mapping(m)
                })
                .collect();
            map.insert(
                serde_yaml::Value::String("seed_entities".into()),
                serde_yaml::to_value(&seed_list).unwrap(),
            );
            let intro_list: Vec<serde_yaml::Value> = detail
                .introduced_entities
                .iter()
                .map(|e| {
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("slug".into()),
                        serde_yaml::Value::String(e.slug.clone()),
                    );
                    m.insert(
                        serde_yaml::Value::String("id".into()),
                        serde_yaml::to_value(&e.id).unwrap(),
                    );
                    serde_yaml::Value::Mapping(m)
                })
                .collect();
            map.insert(
                serde_yaml::Value::String("introduced_entities".into()),
                serde_yaml::to_value(&intro_list).unwrap(),
            );
            serde_yaml::Value::Mapping(map)
        }
        CommandResult::SessionList(sessions) => {
            let items: Vec<serde_yaml::Value> = sessions
                .iter()
                .map(|s| {
                    let mut map = serde_yaml::Mapping::new();
                    map.insert(
                        serde_yaml::Value::String("id".into()),
                        serde_yaml::to_value(&s.id).unwrap(),
                    );
                    map.insert(
                        serde_yaml::Value::String("slug".into()),
                        serde_yaml::Value::String(s.slug.clone()),
                    );
                    if let Some(ref desc) = s.description {
                        map.insert(
                            serde_yaml::Value::String("description".into()),
                            serde_yaml::Value::String(desc.clone()),
                        );
                    }
                    map.insert(
                        serde_yaml::Value::String("entity_type_count".into()),
                        serde_yaml::Value::Number(serde_yaml::Number::from(s.entity_type_count as u64)),
                    );
                    map.insert(
                        serde_yaml::Value::String("seed_count".into()),
                        serde_yaml::Value::Number(serde_yaml::Number::from(s.seed_count as u64)),
                    );
                    map.insert(
                        serde_yaml::Value::String("introduced_count".into()),
                        serde_yaml::Value::Number(serde_yaml::Number::from(s.introduced_count as u64)),
                    );
                    serde_yaml::Value::Mapping(map)
                })
                .collect();
            serde_yaml::to_value(&items).unwrap()
        }
        CommandResult::EntityList(entities) => {
            let items: Vec<serde_yaml::Value> = entities
                .iter()
                .map(|e| {
                    let mut map = serde_yaml::Mapping::new();
                    map.insert(
                        serde_yaml::Value::String("id".into()),
                        serde_yaml::to_value(&e.id).unwrap(),
                    );
                    map.insert(
                        serde_yaml::Value::String("slug".into()),
                        serde_yaml::Value::String(e.slug.clone()),
                    );
                    serde_yaml::Value::Mapping(map)
                })
                .collect();
            serde_yaml::to_value(&items).unwrap()
        }
        CommandResult::EntityDetail(projection) => {
            serde_yaml::to_value(&projection).unwrap()
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
    if let Some(tx_id) = &entry.tx_id {
        map.insert(
            serde_yaml::Value::String("tx_id".into()),
            serde_yaml::to_value(tx_id).unwrap(),
        );
    }
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

fn serialize_entity_result(result: &TransactionEntityResult) -> serde_yaml::Value {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("entity".into()),
        serde_yaml::Value::String(result.entity_slug.clone()),
    );
    map.insert(
        serde_yaml::Value::String("entity_id".into()),
        serde_yaml::to_value(&result.entity_id).unwrap(),
    );
    if !result.facts.is_empty() {
        let items: Vec<serde_yaml::Value> = result.facts.iter().map(serialize_log_entry).collect();
        map.insert(
            serde_yaml::Value::String("facts".into()),
            serde_yaml::to_value(&items).unwrap(),
        );
    }
    if !result.hypotheses.is_empty() {
        let items: Vec<serde_yaml::Value> =
            result.hypotheses.iter().map(serialize_log_entry).collect();
        map.insert(
            serde_yaml::Value::String("hypotheses".into()),
            serde_yaml::to_value(&items).unwrap(),
        );
    }
    serde_yaml::Value::Mapping(map)
}
