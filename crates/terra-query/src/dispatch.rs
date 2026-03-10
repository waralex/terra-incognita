use crate::error::QueryError;
use crate::format::ContentFormat;
use crate::query::{QueryDto, ResponseShape};
use crate::response::{
    AssertedResponse, AttachedResponse, EntityListItem, EntityTypeDetailResponse,
    SessionResponse, TransactionResultResponse,
};
use terra_core::assertion::AssertionStore;
use terra_core::command::CommandResult;
use terra_core::schema::SchemaRegistry;

/// Deserializes a query from bytes, executes it, and serializes the result back to bytes.
///
/// This is the full request cycle without any transport knowledge.
/// The caller (HTTP server, embedded agent, tests) manages locking and lifetime.
pub fn dispatch(
    input: &[u8],
    format: ContentFormat,
    registry: &mut SchemaRegistry,
    store: &AssertionStore,
) -> Result<Vec<u8>, QueryError> {
    let dto: QueryDto = format
        .deserialize(input)
        .map_err(|e| QueryError::bad_request("parse_error", e))?;
    let (cmd, shape) = dto.into_command()?;
    let result = terra_core::command::execute(cmd, registry, store)?;
    let value = serialize_result(result, shape);
    Ok(format.serialize_value(&value))
}

fn serialize_result(result: CommandResult, shape: ResponseShape) -> serde_json::Value {
    match result {
        CommandResult::EntityTypes(types) => match shape {
            ResponseShape::Single => serde_json::to_value(&types[0]).unwrap(),
            ResponseShape::Batch => serde_json::to_value(&types).unwrap(),
        },
        CommandResult::Properties(props) => match shape {
            ResponseShape::Single => serde_json::to_value(&props[0]).unwrap(),
            ResponseShape::Batch => serde_json::to_value(&props).unwrap(),
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
            serde_json::to_value(&resp).unwrap()
        }
        CommandResult::Asserted {
            transaction,
            facts,
            hypotheses,
        } => serde_json::to_value(&AssertedResponse {
            tx_id: transaction.id,
            facts,
            hypotheses,
        })
        .unwrap(),
        CommandResult::EntityTypeDetail {
            entity_type,
            properties,
        } => serde_json::to_value(&EntityTypeDetailResponse {
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
        } => serde_json::to_value(&TransactionResultResponse {
            tx_id: transaction.id,
            introduce: introduced,
            asserts: asserted,
        })
        .unwrap(),
        CommandResult::Session(detail) => {
            serde_json::to_value(&SessionResponse::from(detail)).unwrap()
        }
        CommandResult::SessionList(sessions) => serde_json::to_value(&sessions).unwrap(),
        CommandResult::EntityList(entities) => {
            let items: Vec<EntityListItem> = entities
                .into_iter()
                .map(|e| EntityListItem {
                    id: e.id,
                    slug: e.slug,
                })
                .collect();
            serde_json::to_value(&items).unwrap()
        }
        CommandResult::EntityDetail(projection) => serde_json::to_value(&projection).unwrap(),
        CommandResult::LogEntries(entries) => serde_json::to_value(&entries).unwrap(),
    }
}
