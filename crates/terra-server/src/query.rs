use crate::error::ApiError;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "command")]
pub enum Command {
    #[serde(rename = "entity-type.create")]
    CreateEntityType {
        slug: String,
        description: Option<String>,
    },
    #[serde(rename = "entity-type.list")]
    ListEntityTypes,
    #[serde(rename = "entity-type.get")]
    GetEntityType {
        slug: String,
    },
    #[serde(rename = "property.create")]
    CreateProperty {
        slug: String,
        value_type: terra_core::schema::ValueType,
        description: Option<String>,
    },
    #[serde(rename = "property.list")]
    ListProperties {
        entity_type: Option<String>,
    },
    #[serde(rename = "property.attach")]
    AttachProperty {
        entity_type: String,
        slug: String,
    },
    #[serde(rename = "entity.create")]
    CreateEntity {
        entity_type: String,
        name: String,
        kind: Option<terra_core::assertion::AssertionKind>,
        context: Option<serde_yaml::Value>,
    },
}

impl Command {
    pub fn parse(body: &[u8]) -> Result<Self, ApiError> {
        serde_yaml::from_slice(body)
            .map_err(|e| ApiError::bad_request("parse_error", e.to_string()))
    }
}
