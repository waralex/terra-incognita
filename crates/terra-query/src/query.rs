use std::collections::HashMap;

use crate::error::QueryError;
use serde::Deserialize;
use terra_core::assertion::{PropertyValue, RangeValue, SetValue, StructValue};
use terra_core::command::{
    AddProperties, AssertItem, AssertionItem, Command, CreateBranchInput,
    CreateEntityType, CreatePropertyDef, HideUnhideInput, IntroduceItem,
    TaskCloseItem, TaskCreateItem, TaskUpdateItem,
    TransactionInput,
};

/// DTO for inline property definition (within entity type creation or add_properties).
#[derive(Deserialize)]
pub struct PropertyDefDto {
    pub slug: String,
    pub value_type: terra_core::schema::ValueType,
    pub description: Option<String>,
}

/// DTO for batch entity type creation items.
#[derive(Deserialize)]
pub struct EntityTypeItemDto {
    pub slug: String,
    pub description: Option<String>,
    #[serde(default)]
    pub properties: Vec<PropertyDefDto>,
}

/// DTO for adding properties to an existing entity type.
#[derive(Deserialize)]
pub struct AddPropertiesDto {
    pub entity_type: String,
    pub properties: Vec<PropertyDefDto>,
}

/// DTO for a single assertion item (fact or hypothesis).
#[derive(Deserialize)]
pub struct AssertionItemDto {
    #[serde(default)]
    pub properties: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub reasoning: serde_json::Value,
}

/// DTO for an introduce item in a transaction.
#[derive(Deserialize)]
pub struct IntroduceItemDto {
    pub entity: String,
    pub entity_type: String,
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

/// DTO for hide/unhide input.
#[derive(Deserialize, Default)]
pub struct HideUnhideDto {
    #[serde(default)]
    pub entities: Vec<String>,
    #[serde(default)]
    pub entity_types: Vec<String>,
    #[serde(default)]
    pub properties: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<String>,
}

/// DTO for creating a task.
#[derive(Deserialize)]
pub struct TaskCreateDto {
    pub slug: String,
    #[serde(default)]
    pub goal: serde_json::Value,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub context: serde_json::Value,
    pub kind: Option<String>,
}

/// DTO for updating a task's notes.
#[derive(Deserialize)]
pub struct TaskUpdateDto {
    pub slug: String,
    #[serde(default)]
    pub notes: serde_json::Value,
}

/// DTO for closing a task.
#[derive(Deserialize)]
pub struct TaskCloseDto {
    pub slug: String,
    #[serde(default)]
    pub resolution: serde_json::Value,
}

/// Serde-tagged query DTO. Normalized into a domain [`Command`] via [`into_command`](QueryDto::into_command).
#[derive(Deserialize)]
#[serde(tag = "command")]
pub enum QueryDto {
    #[serde(rename = "entity-type.list")]
    ListEntityTypes,
    #[serde(rename = "property.list")]
    ListProperties { entity_type: Option<String> },
    #[serde(rename = "entity.list")]
    ListEntities,
    #[serde(rename = "transaction")]
    Transaction {
        #[serde(default)]
        reasoning: serde_json::Value,
        #[serde(default)]
        question: Option<String>,
        #[serde(default)]
        answer: Option<String>,
        #[serde(default)]
        entity_types: Vec<EntityTypeItemDto>,
        #[serde(default)]
        add_properties: Vec<AddPropertiesDto>,
        #[serde(default)]
        hide: HideUnhideDto,
        #[serde(default)]
        unhide: HideUnhideDto,
        #[serde(default)]
        introduce: Vec<IntroduceItemDto>,
        #[serde(default)]
        asserts: Vec<AssertItemDto>,
        #[serde(default)]
        commands: Vec<serde_json::Value>,
        #[serde(default)]
        tasks: Vec<TaskCreateDto>,
        #[serde(default)]
        update_tasks: Vec<TaskUpdateDto>,
        #[serde(default)]
        close_tasks: Vec<TaskCloseDto>,
    },
    #[serde(rename = "branch.create")]
    CreateBranch {
        slug: String,
        #[serde(default)]
        reasoning: serde_json::Value,
        #[serde(default)]
        parent: String,
        from_tx: Option<String>,
    },
    #[serde(rename = "branch.get")]
    GetBranch { slug: String },
    #[serde(rename = "branch.list")]
    ListBranches,
    #[serde(rename = "log.list")]
    ListLog,
    #[serde(rename = "branch.state")]
    BranchState {
        slug: String,
        #[serde(default = "default_last_transactions")]
        last_transactions: usize,
        transaction_id: Option<String>,
    },
}

fn default_last_transactions() -> usize {
    10
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
            QueryDto::ListEntityTypes => {
                Ok((Command::ListEntityTypes, ResponseShape::Batch))
            }
            QueryDto::ListProperties { entity_type } => Ok((
                Command::ListProperties { entity_type },
                ResponseShape::Batch,
            )),
            QueryDto::ListEntities => {
                Ok((Command::ListEntities, ResponseShape::Batch))
            }
            QueryDto::Transaction {
                reasoning,
                question,
                answer,
                entity_types,
                add_properties,
                hide,
                unhide,
                introduce,
                asserts,
                commands,
                tasks,
                update_tasks,
                close_tasks,
            } => {
                let entity_type_items = entity_types
                    .into_iter()
                    .map(|item| CreateEntityType {
                        slug: item.slug,
                        description: item.description,
                        properties: item.properties
                            .into_iter()
                            .map(|p| CreatePropertyDef {
                                slug: p.slug,
                                value_type: p.value_type,
                                description: p.description,
                            })
                            .collect(),
                    })
                    .collect();
                let add_property_items = add_properties
                    .into_iter()
                    .map(|item| AddProperties {
                        entity_type: item.entity_type,
                        properties: item.properties
                            .into_iter()
                            .map(|p| CreatePropertyDef {
                                slug: p.slug,
                                value_type: p.value_type,
                                description: p.description,
                            })
                            .collect(),
                    })
                    .collect();
                let introduce_items = introduce
                    .into_iter()
                    .map(|item| {
                        Ok(IntroduceItem {
                            entity: item.entity,
                            entity_type: item.entity_type,
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
                        question,
                        answer,
                        entity_types: entity_type_items,
                        add_properties: add_property_items,
                        hide: HideUnhideInput {
                            entities: hide.entities,
                            entity_types: hide.entity_types,
                            properties: hide.properties,
                            tasks: hide.tasks,
                        },
                        unhide: HideUnhideInput {
                            entities: unhide.entities,
                            entity_types: unhide.entity_types,
                            properties: unhide.properties,
                            tasks: unhide.tasks,
                        },
                        introduce: introduce_items,
                        asserts: assert_items,
                        commands,
                        tasks: tasks
                            .into_iter()
                            .map(|item| TaskCreateItem {
                                slug: item.slug,
                                goal: item.goal,
                                reasoning: item.reasoning,
                                context: item.context,
                                kind: item.kind,
                            })
                            .collect(),
                        update_tasks: update_tasks
                            .into_iter()
                            .map(|item| TaskUpdateItem {
                                slug: item.slug,
                                notes: item.notes,
                            })
                            .collect(),
                        close_tasks: close_tasks
                            .into_iter()
                            .map(|item| TaskCloseItem {
                                slug: item.slug,
                                resolution: item.resolution,
                            })
                            .collect(),
                    }),
                    ResponseShape::Single,
                ))
            }
            QueryDto::CreateBranch {
                slug,
                reasoning,
                parent,
                from_tx,
            } => {
                let from_tx = from_tx
                    .map(|s| uuid::Uuid::parse_str(&s))
                    .transpose()
                    .map_err(|e| QueryError::bad_request("parse_error", format!("invalid from_tx UUID: {e}")))?;
                Ok((
                    Command::CreateBranch(CreateBranchInput {
                        slug,
                        reasoning,
                        parent,
                        from_tx,
                    }),
                    ResponseShape::Single,
                ))
            }
            QueryDto::GetBranch { slug } => {
                Ok((Command::GetBranch { slug }, ResponseShape::Single))
            }
            QueryDto::ListBranches => Ok((Command::ListBranches, ResponseShape::Batch)),
            QueryDto::ListLog => Ok((Command::ListLog, ResponseShape::Batch)),
            QueryDto::BranchState { slug, last_transactions, transaction_id } => {
                let at_tx = transaction_id
                    .map(|s| uuid::Uuid::parse_str(&s))
                    .transpose()
                    .map_err(|e| QueryError::bad_request("parse_error", format!("invalid transaction_id UUID: {e}")))?;
                Ok((
                    Command::BranchState { slug, last_transactions, at_tx },
                    ResponseShape::Single,
                ))
            }
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
