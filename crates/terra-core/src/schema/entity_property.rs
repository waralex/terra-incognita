use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Semantic value type that a property carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    /// Classification via membership assertions.
    Set,
    /// Structured or composite value.
    Struct,
    /// Numeric range.
    Range,
}

impl ValueType {
    /// Returns the string representation (`"set"`, `"struct"`, `"range"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            ValueType::Set => "set",
            ValueType::Struct => "struct",
            ValueType::Range => "range",
        }
    }

    /// Parses a value type from its string representation.
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

/// Registered property that can be attached to entity types.
#[derive(Debug, Clone, Serialize)]
pub struct EntityProperty {
    pub id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub value_type: ValueType,
    pub created_at: DateTime<Utc>,
}
