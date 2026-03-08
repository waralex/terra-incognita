use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    Set,
    Struct,
    Range,
}

impl ValueType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ValueType::Set => "set",
            ValueType::Struct => "struct",
            ValueType::Range => "range",
        }
    }

    pub fn from_str(s: &str) -> Option<ValueType> {
        match s {
            "set" => Some(ValueType::Set),
            "struct" => Some(ValueType::Struct),
            "range" => Some(ValueType::Range),
            _ => None,
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityProperty {
    pub id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub value_type: ValueType,
    pub created_at: DateTime<Utc>,
}
