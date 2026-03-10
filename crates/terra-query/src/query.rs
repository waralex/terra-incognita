use std::collections::HashMap;

use crate::error::QueryError;
use serde::Deserialize;
use terra_core::assertion::{PropertyValue, RangeValue, SetValue, StructValue};
use terra_core::command::{
    AssertEntityInput, AssertItem, AssertionItem, AttachProperty, Command, CreateBranchInput,
    CreateEntityType, CreateProperty, IntroduceItem, TransactionInput,
};

/// DTO for batch entity type creation items.
#[derive(Deserialize)]
pub struct EntityTypeItemDto {
    pub slug: String,
    pub description: Option<String>,
    #[serde(default)]
    pub properties: Vec<String>,
}

/// DTO for batch property creation items.
#[derive(Deserialize)]
pub struct PropertyItemDto {
    pub slug: String,
    pub value_type: terra_core::schema::ValueType,
    pub description: Option<String>,
    #[serde(default)]
    pub entity_types: Vec<String>,
}

/// DTO for batch property attachment items.
#[derive(Deserialize)]
pub struct AttachItemDto {
    pub entity_type: String,
    pub slug: String,
}

/// DTO for a single assertion item (fact or hypothesis).
#[derive(Deserialize)]
pub struct AssertionItemDto {
    pub entity_type: String,
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub reasoning: serde_json::Value,
}

/// DTO for an introduce item in a transaction.
#[derive(Deserialize)]
pub struct IntroduceItemDto {
    pub entity: String,
    pub description: Option<String>,
    #[serde(default)]
    pub facts: Vec<AssertionItemDto>,
    #[serde(default)]
    pub hypotheses: Vec<AssertionItemDto>,
}

/// DTO for an assert item in a transaction.
#[derive(Deserialize)]
pub struct AssertItemDto {
    pub entity: String,
    #[serde(default)]
    pub facts: Vec<AssertionItemDto>,
    #[serde(default)]
    pub hypotheses: Vec<AssertionItemDto>,
}

/// Serde-tagged query DTO. Normalized into a domain [`Command`] via [`into_command`](QueryDto::into_command).
#[derive(Deserialize)]
#[serde(tag = "command")]
pub enum QueryDto {
    #[serde(rename = "entity-type.create")]
    CreateEntityType {
        slug: Option<String>,
        description: Option<String>,
        #[serde(default)]
        properties: Vec<String>,
        items: Option<Vec<EntityTypeItemDto>>,
    },
    #[serde(rename = "entity-type.list")]
    ListEntityTypes,
    #[serde(rename = "entity-type.get")]
    GetEntityType { slug: String },
    #[serde(rename = "property.create")]
    CreateProperty {
        slug: Option<String>,
        value_type: Option<terra_core::schema::ValueType>,
        description: Option<String>,
        #[serde(default)]
        entity_types: Vec<String>,
        items: Option<Vec<PropertyItemDto>>,
    },
    #[serde(rename = "property.list")]
    ListProperties { entity_type: Option<String> },
    #[serde(rename = "property.attach")]
    AttachProperty {
        entity_type: Option<String>,
        slug: Option<String>,
        items: Option<Vec<AttachItemDto>>,
    },
    #[serde(rename = "entity.create")]
    CreateEntity {
        entity: String,
        description: Option<String>,
        #[serde(default)]
        reasoning: serde_json::Value,
        #[serde(default)]
        facts: Vec<AssertionItemDto>,
        #[serde(default)]
        hypotheses: Vec<AssertionItemDto>,
    },
    #[serde(rename = "entity.assert")]
    AssertEntity {
        entity: String,
        #[serde(default)]
        reasoning: serde_json::Value,
        #[serde(default)]
        facts: Vec<AssertionItemDto>,
        #[serde(default)]
        hypotheses: Vec<AssertionItemDto>,
    },
    #[serde(rename = "entity.list")]
    ListEntities,
    #[serde(rename = "entity.get")]
    GetEntity {
        entity: String,
        entity_type: String,
    },
    #[serde(rename = "transaction")]
    Transaction {
        #[serde(default)]
        reasoning: serde_json::Value,
        #[serde(default)]
        introduce: Vec<IntroduceItemDto>,
        #[serde(default)]
        asserts: Vec<AssertItemDto>,
    },
    #[serde(rename = "branch.create")]
    CreateBranch {
        slug: String,
        description: Option<String>,
        #[serde(default)]
        parent: String,
        #[serde(default)]
        entities: Vec<String>,
    },
    #[serde(rename = "branch.get")]
    GetBranch { slug: String },
    #[serde(rename = "branch.list")]
    ListBranches,
    #[serde(rename = "log.list")]
    ListLog,
}

/// Controls how the result is serialized back: single object or array.
pub enum ResponseShape {
    /// DTO had inline fields (single item) — serialize `results[0]` as object.
    Single,
    /// DTO had `items` array (batch) — serialize full array.
    Batch,
}

impl QueryDto {
    /// Normalizes the DTO into a domain command and response shape.
    pub fn into_command(self) -> Result<(Command, ResponseShape), QueryError> {
        match self {
            QueryDto::CreateEntityType {
                slug,
                description,
                properties,
                items,
            } => match (slug, items) {
                (Some(slug), None) => Ok((
                    Command::CreateEntityTypes(vec![CreateEntityType {
                        slug,
                        description,
                        properties,
                    }]),
                    ResponseShape::Single,
                )),
                (None, Some(batch_items)) => Ok((
                    Command::CreateEntityTypes(
                        batch_items
                            .into_iter()
                            .map(|item| CreateEntityType {
                                slug: item.slug,
                                description: item.description,
                                properties: item.properties,
                            })
                            .collect(),
                    ),
                    ResponseShape::Batch,
                )),
                _ => Err(QueryError::bad_request(
                    "parse_error",
                    "provide either 'slug' for single creation or 'items' for batch creation, not both",
                )),
            },
            QueryDto::ListEntityTypes => {
                Ok((Command::ListEntityTypes, ResponseShape::Batch))
            }
            QueryDto::GetEntityType { slug } => {
                Ok((Command::GetEntityType { slug }, ResponseShape::Single))
            }
            QueryDto::CreateProperty {
                slug,
                value_type,
                description,
                entity_types,
                items,
            } => match (slug, items) {
                (Some(slug), None) => {
                    let value_type = value_type.ok_or_else(|| {
                        QueryError::bad_request("parse_error", "value_type is required")
                    })?;
                    Ok((
                        Command::CreateProperties(vec![CreateProperty {
                            slug,
                            value_type,
                            description,
                            entity_types,
                        }]),
                        ResponseShape::Single,
                    ))
                }
                (None, Some(batch_items)) => Ok((
                    Command::CreateProperties(
                        batch_items
                            .into_iter()
                            .map(|item| CreateProperty {
                                slug: item.slug,
                                value_type: item.value_type,
                                description: item.description,
                                entity_types: item.entity_types,
                            })
                            .collect(),
                    ),
                    ResponseShape::Batch,
                )),
                _ => Err(QueryError::bad_request(
                    "parse_error",
                    "provide either 'slug' for single creation or 'items' for batch creation, not both",
                )),
            },
            QueryDto::ListProperties { entity_type } => Ok((
                Command::ListProperties { entity_type },
                ResponseShape::Batch,
            )),
            QueryDto::AttachProperty {
                entity_type,
                slug,
                items,
            } => match (entity_type.zip(slug), items) {
                (Some((et, slug)), None) => Ok((
                    Command::AttachProperties(vec![AttachProperty {
                        entity_type: et,
                        property: slug,
                    }]),
                    ResponseShape::Single,
                )),
                (None, Some(batch_items)) => Ok((
                    Command::AttachProperties(
                        batch_items
                            .into_iter()
                            .map(|item| AttachProperty {
                                entity_type: item.entity_type,
                                property: item.slug,
                            })
                            .collect(),
                    ),
                    ResponseShape::Batch,
                )),
                _ => Err(QueryError::bad_request(
                    "parse_error",
                    "provide either 'entity_type'+'slug' for single attach or 'items' for batch, not both",
                )),
            },
            QueryDto::CreateEntity {
                entity,
                description,
                reasoning,
                facts,
                hypotheses,
            } => Ok((
                Command::CreateEntity(AssertEntityInput {
                    entity,
                    description,
                    reasoning,
                    facts: convert_assertion_items(facts)?,
                    hypotheses: convert_assertion_items(hypotheses)?,
                }),
                ResponseShape::Single,
            )),
            QueryDto::AssertEntity {
                entity,
                reasoning,
                facts,
                hypotheses,
            } => Ok((
                Command::AssertEntity(AssertEntityInput {
                    entity,
                    description: None,
                    reasoning,
                    facts: convert_assertion_items(facts)?,
                    hypotheses: convert_assertion_items(hypotheses)?,
                }),
                ResponseShape::Single,
            )),
            QueryDto::ListEntities => {
                Ok((Command::ListEntities, ResponseShape::Batch))
            }
            QueryDto::GetEntity {
                entity,
                entity_type,
            } => Ok((
                Command::GetEntity {
                    entity,
                    entity_type,
                },
                ResponseShape::Single,
            )),
            QueryDto::Transaction {
                reasoning,
                introduce,
                asserts,
            } => {
                let introduce_items = introduce
                    .into_iter()
                    .map(|item| {
                        Ok(IntroduceItem {
                            entity: item.entity,
                            description: item.description,
                            facts: convert_assertion_items(item.facts)?,
                            hypotheses: convert_assertion_items(item.hypotheses)?,
                        })
                    })
                    .collect::<Result<Vec<_>, QueryError>>()?;
                let assert_items = asserts
                    .into_iter()
                    .map(|item| {
                        Ok(AssertItem {
                            entity: item.entity,
                            facts: convert_assertion_items(item.facts)?,
                            hypotheses: convert_assertion_items(item.hypotheses)?,
                        })
                    })
                    .collect::<Result<Vec<_>, QueryError>>()?;
                Ok((
                    Command::Transaction(TransactionInput {
                        reasoning,
                        introduce: introduce_items,
                        asserts: assert_items,
                    }),
                    ResponseShape::Single,
                ))
            }
            QueryDto::CreateBranch {
                slug,
                description,
                parent,
                entities,
            } => Ok((
                Command::CreateBranch(CreateBranchInput {
                    slug,
                    description,
                    parent,
                    entities,
                }),
                ResponseShape::Single,
            )),
            QueryDto::GetBranch { slug } => {
                Ok((Command::GetBranch { slug }, ResponseShape::Single))
            }
            QueryDto::ListBranches => Ok((Command::ListBranches, ResponseShape::Batch)),
            QueryDto::ListLog => Ok((Command::ListLog, ResponseShape::Batch)),
        }
    }
}

fn convert_assertion_items(
    items: Vec<AssertionItemDto>,
) -> Result<Vec<AssertionItem>, QueryError> {
    items
        .into_iter()
        .map(|item| {
            let properties = item
                .properties
                .into_iter()
                .map(|(slug, val)| {
                    let pv = parse_property_value(val)?;
                    Ok((slug, pv))
                })
                .collect::<Result<HashMap<_, _>, QueryError>>()?;

            Ok(AssertionItem {
                entity_type: item.entity_type,
                properties,
                reasoning: item.reasoning,
            })
        })
        .collect()
}

/// Parses a JSON property value into a typed PropertyValue.
///
/// Expected formats:
/// - Set: `{contains: [...], not_contains: [...]}`
/// - Range: `{eq: V}`, `{from: V, to: V}`, `{from: V}`, `{to: V}`
/// - Struct: any other mapping or scalar
fn parse_property_value(val: serde_json::Value) -> Result<PropertyValue, QueryError> {
    if let serde_json::Value::Object(ref map) = val {
        let has_contains = map.contains_key("contains");
        let has_not_contains = map.contains_key("not_contains");
        if has_contains || has_not_contains {
            let contains = map
                .get("contains")
                .and_then(|v| v.as_array())
                .map(|arr| arr.clone())
                .unwrap_or_default();
            let not_contains = map
                .get("not_contains")
                .and_then(|v| v.as_array())
                .map(|arr| arr.clone())
                .unwrap_or_default();
            return Ok(PropertyValue::Set(SetValue {
                contains,
                not_contains,
            }));
        }

        let has_eq = map.contains_key("eq");
        let has_from = map.contains_key("from");
        let has_to = map.contains_key("to");

        if has_eq {
            let val = map.get("eq").cloned().unwrap_or(serde_json::Value::Null);
            return Ok(PropertyValue::Range(RangeValue::Eq(val)));
        }
        if has_from && has_to {
            let from = map.get("from").cloned().unwrap_or(serde_json::Value::Null);
            let to = map.get("to").cloned().unwrap_or(serde_json::Value::Null);
            return Ok(PropertyValue::Range(RangeValue::Between { from, to }));
        }
        if has_from {
            let val = map.get("from").cloned().unwrap_or(serde_json::Value::Null);
            return Ok(PropertyValue::Range(RangeValue::From(val)));
        }
        if has_to {
            let val = map.get("to").cloned().unwrap_or(serde_json::Value::Null);
            return Ok(PropertyValue::Range(RangeValue::To(val)));
        }
    }

    Ok(PropertyValue::Struct(StructValue(val)))
}
