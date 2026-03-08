use crate::error::ApiError;
use serde::Deserialize;
use terra_core::command::{
    AttachProperty, Command, CreateEntity, CreateEntityType, CreateProperty,
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

/// DTO for batch entity creation items.
#[derive(Deserialize)]
pub struct EntityItemDto {
    pub entity_name: String,
    pub entity_type: Option<String>,
    pub context: Option<serde_yaml::Value>,
}

/// Serde-tagged query DTO parsed from YAML. Normalized into a domain [`Command`] via [`into_command`](QueryDto::into_command).
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
        entity_name: Option<String>,
        entity_type: Option<String>,
        context: Option<serde_yaml::Value>,
        items: Option<Vec<EntityItemDto>>,
    },
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
    /// Parses a YAML request body into a query DTO.
    pub fn parse(body: &[u8]) -> Result<Self, ApiError> {
        serde_yaml::from_slice(body)
            .map_err(|e| ApiError::bad_request("parse_error", e.to_string()))
    }

    /// Normalizes the DTO into a domain command and response shape.
    pub fn into_command(self) -> Result<(Command, ResponseShape), ApiError> {
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
                _ => Err(ApiError::bad_request(
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
                        ApiError::bad_request("parse_error", "value_type is required")
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
                _ => Err(ApiError::bad_request(
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
                _ => Err(ApiError::bad_request(
                    "parse_error",
                    "provide either 'entity_type'+'slug' for single attach or 'items' for batch, not both",
                )),
            },
            QueryDto::CreateEntity {
                entity_name,
                entity_type,
                context,
                items,
            } => match (entity_name, items) {
                (Some(name), None) => Ok((
                    Command::CreateEntities(vec![CreateEntity {
                        entity_name: name,
                        entity_type,
                        context: yaml_context_to_json(context),
                    }]),
                    ResponseShape::Single,
                )),
                (None, Some(batch_items)) => Ok((
                    Command::CreateEntities(
                        batch_items
                            .into_iter()
                            .map(|item| CreateEntity {
                                entity_name: item.entity_name,
                                entity_type: item.entity_type,
                                context: yaml_context_to_json(item.context),
                            })
                            .collect(),
                    ),
                    ResponseShape::Batch,
                )),
                _ => Err(ApiError::bad_request(
                    "parse_error",
                    "provide either 'entity_name' for single creation or 'items' for batch, not both",
                )),
            },
            QueryDto::ListLog => Ok((Command::ListLog, ResponseShape::Batch)),
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
