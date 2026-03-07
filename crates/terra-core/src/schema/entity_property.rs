use chrono::{DateTime, Utc};
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    String,
    Number,
}

impl ValueType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ValueType::String => "string",
            ValueType::Number => "number",
        }
    }

    pub fn from_str(s: &str) -> Option<ValueType> {
        match s {
            "string" => Some(ValueType::String),
            "number" => Some(ValueType::Number),
            _ => None,
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct EntityProperty {
    pub id: Uuid,
    pub slug: String,
    pub value_type: ValueType,
    pub created_at: DateTime<Utc>,
}
