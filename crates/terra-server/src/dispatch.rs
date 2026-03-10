use crate::error::ApiError;
use crate::query::{QueryDto, ResponseShape};
use crate::response::{
    AssertedResponse, AttachedResponse, EntityListItem, EntityTypeDetailResponse,
    SessionResponse, TransactionResultResponse,
};
use crate::state::AppState;
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
            let resp = AttachedResponse {
                status: "ok",
                count: if matches!(shape, ResponseShape::Batch) {
                    Some(count)
                } else {
                    None
                },
            };
            serde_yaml::to_value(&resp).unwrap()
        }
        CommandResult::Asserted {
            transaction,
            facts,
            hypotheses,
        } => serde_yaml::to_value(&AssertedResponse {
            tx_id: transaction.id,
            facts,
            hypotheses,
        })
        .unwrap(),
        CommandResult::EntityTypeDetail {
            entity_type,
            properties,
        } => serde_yaml::to_value(&EntityTypeDetailResponse {
            id: entity_type.id,
            slug: entity_type.slug,
            description: entity_type.description,
            created_at: entity_type.created_at,
            properties,
        })
        .unwrap(),
        CommandResult::TransactionResult {
            transaction,
            introduced,
            asserted,
        } => serde_yaml::to_value(&TransactionResultResponse {
            tx_id: transaction.id,
            introduce: introduced,
            asserts: asserted,
        })
        .unwrap(),
        CommandResult::Session(detail) => {
            serde_yaml::to_value(&SessionResponse::from(detail)).unwrap()
        }
        CommandResult::SessionList(sessions) => serde_yaml::to_value(&sessions).unwrap(),
        CommandResult::EntityList(entities) => {
            let items: Vec<EntityListItem> = entities
                .into_iter()
                .map(|e| EntityListItem {
                    id: e.id,
                    slug: e.slug,
                })
                .collect();
            serde_yaml::to_value(&items).unwrap()
        }
        CommandResult::EntityDetail(projection) => serde_yaml::to_value(&projection).unwrap(),
        CommandResult::LogEntries(entries) => serde_yaml::to_value(&entries).unwrap(),
    }
}
