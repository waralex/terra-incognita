use crate::error::ApiError;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct EntityTypeItem {
    pub slug: String,
    pub description: Option<String>,
    #[serde(default)]
    pub properties: Vec<String>,
}

#[derive(Deserialize)]
pub struct PropertyItem {
    pub slug: String,
    pub value_type: terra_core::schema::ValueType,
    pub description: Option<String>,
    #[serde(default)]
    pub entity_types: Vec<String>,
}

#[derive(Deserialize)]
pub struct AttachItem {
    pub entity_type: String,
    pub slug: String,
}

#[derive(Deserialize)]
pub struct EntityItem {
    pub entity_name: String,
    pub entity_type: Option<String>,
    pub context: Option<serde_yaml::Value>,
}

#[derive(Deserialize)]
#[serde(tag = "command")]
pub enum Command {
    #[serde(rename = "entity-type.create")]
    CreateEntityType {
        slug: Option<String>,
        description: Option<String>,
        #[serde(default)]
        properties: Vec<String>,
        items: Option<Vec<EntityTypeItem>>,
    },
    #[serde(rename = "entity-type.list")]
    ListEntityTypes,
    #[serde(rename = "entity-type.get")]
    GetEntityType {
        slug: String,
    },
    #[serde(rename = "property.create")]
    CreateProperty {
        slug: Option<String>,
        value_type: Option<terra_core::schema::ValueType>,
        description: Option<String>,
        #[serde(default)]
        entity_types: Vec<String>,
        items: Option<Vec<PropertyItem>>,
    },
    #[serde(rename = "property.list")]
    ListProperties {
        entity_type: Option<String>,
    },
    #[serde(rename = "property.attach")]
    AttachProperty {
        entity_type: Option<String>,
        slug: Option<String>,
        items: Option<Vec<AttachItem>>,
    },
    #[serde(rename = "entity.create")]
    CreateEntity {
        entity_name: Option<String>,
        entity_type: Option<String>,
        context: Option<serde_yaml::Value>,
        items: Option<Vec<EntityItem>>,
    },
    #[serde(rename = "log.list")]
    ListLog,
}

impl Command {
    pub fn parse(body: &[u8]) -> Result<Self, ApiError> {
        serde_yaml::from_slice(body)
            .map_err(|e| ApiError::bad_request("parse_error", e.to_string()))
    }
}
