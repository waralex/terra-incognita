use crate::error::ApiError;

pub enum Command {
    CreateEntityType { slug: String, description: Option<String> },
    ListEntityTypes,
    GetEntityType { slug: String },
    CreateProperty { slug: String, value_type: terra_core::schema::ValueType, description: Option<String> },
    ListProperties { entity_type: Option<String> },
    AttachProperty { entity_type: String, slug: String },
    CreateEntity {
        entity_type: String,
        name: String,
        kind: Option<terra_core::assertion::AssertionKind>,
        context: Option<serde_yaml::Value>,
    },
}

impl Command {
    pub fn parse(body: &[u8]) -> Result<Self, ApiError> {
        let val: serde_yaml::Value = serde_yaml::from_slice(body)
            .map_err(|e| ApiError::bad_request("parse_error", e.to_string()))?;

        let verb = val
            .get("verb")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::bad_request("parse_error", "missing field: verb"))?;

        let target = val
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::bad_request("parse_error", "missing field: target"))?;

        match (verb, target) {
            ("create", "entity-type") => {
                let slug = require_str(&val, "slug")?;
                let description = optional_str(&val, "description");
                Ok(Command::CreateEntityType { slug, description })
            }
            ("list", "entity-type") => Ok(Command::ListEntityTypes),
            ("get", "entity-type") => {
                let slug = require_str(&val, "slug")?;
                Ok(Command::GetEntityType { slug })
            }
            ("create", "property") => {
                let slug = require_str(&val, "slug")?;
                let vt_str = require_str(&val, "value_type")?;
                let value_type: terra_core::schema::ValueType =
                    serde_yaml::from_value(serde_yaml::Value::String(vt_str))
                        .map_err(|_| {
                            ApiError::bad_request(
                                "parse_error",
                                "invalid value_type: expected 'string', 'number', 'struct', or 'set'",
                            )
                        })?;
                let description = optional_str(&val, "description");
                Ok(Command::CreateProperty { slug, value_type, description })
            }
            ("list", "property") => {
                let entity_type = val.get("entity_type").and_then(|v| v.as_str()).map(String::from);
                Ok(Command::ListProperties { entity_type })
            }
            ("attach", "property") => {
                let entity_type = require_str(&val, "entity_type")?;
                let slug = require_str(&val, "slug")?;
                Ok(Command::AttachProperty { entity_type, slug })
            }
            ("create", "entity") => {
                let entity_type = require_str(&val, "entity_type")?;
                let name = require_str(&val, "name")?;
                let kind = match optional_str(&val, "kind").as_deref() {
                    Some("hypothesis") | None => None,
                    Some("refinement") => Some(terra_core::assertion::AssertionKind::Refinement),
                    Some(other) => {
                        return Err(ApiError::bad_request(
                            "parse_error",
                            format!("invalid kind: {other}, expected 'hypothesis' or 'refinement'"),
                        ));
                    }
                };
                let context = val.get("context").cloned();
                Ok(Command::CreateEntity { entity_type, name, kind, context })
            }
            _ => Err(ApiError::bad_request(
                "unknown_command",
                format!("unknown command: {verb} {target}"),
            )),
        }
    }
}

fn require_str(val: &serde_yaml::Value, field: &str) -> Result<String, ApiError> {
    val.get(field)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| ApiError::bad_request("parse_error", format!("missing field: {field}")))
}

fn optional_str(val: &serde_yaml::Value, field: &str) -> Option<String> {
    val.get(field).and_then(|v| v.as_str()).map(String::from)
}
