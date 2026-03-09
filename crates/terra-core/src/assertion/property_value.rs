use serde::{Deserialize, Serialize};

use super::log::LogError;

/// A set membership assertion: which values are included and which are excluded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetValue {
    /// Values asserted to be in the set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contains: Vec<serde_json::Value>,
    /// Values asserted to NOT be in the set.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub not_contains: Vec<serde_json::Value>,
}

/// A numeric/ordinal range assertion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeValue {
    /// Exact value.
    Eq(serde_json::Value),
    /// Closed range [from, to].
    Between {
        from: serde_json::Value,
        to: serde_json::Value,
    },
    /// Open-ended: value >= from.
    From(serde_json::Value),
    /// Open-ended: value <= to.
    To(serde_json::Value),
}

/// A structured value — arbitrary JSON, passed through as-is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructValue(pub serde_json::Value);

/// Typed property value matching one of the three column kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// Set membership assertion.
    Set(SetValue),
    /// Structured/composite value.
    Struct(StructValue),
    /// Range or exact numeric/ordinal value.
    Range(RangeValue),
}

impl PropertyValue {
    /// Serializes the inner value to JSON bytes for column storage.
    pub fn to_bytes(&self) -> Result<Vec<u8>, LogError> {
        let bytes = match self {
            PropertyValue::Set(v) => serde_json::to_vec(v),
            PropertyValue::Struct(v) => serde_json::to_vec(v),
            PropertyValue::Range(v) => serde_json::to_vec(v),
        };
        bytes.map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Deserializes from JSON bytes, given the expected column kind.
    pub fn from_bytes(bytes: &[u8], value_type: crate::schema::ValueType) -> Result<Self, LogError> {
        let parse_err = |e: serde_json::Error| LogError::Storage(e.to_string());
        match value_type {
            crate::schema::ValueType::Set => {
                Ok(PropertyValue::Set(serde_json::from_slice(bytes).map_err(parse_err)?))
            }
            crate::schema::ValueType::Struct => {
                Ok(PropertyValue::Struct(serde_json::from_slice(bytes).map_err(parse_err)?))
            }
            crate::schema::ValueType::Range => {
                Ok(PropertyValue::Range(serde_json::from_slice(bytes).map_err(parse_err)?))
            }
        }
    }

    /// Converts to a generic JSON value (for log body storage).
    pub fn to_json(&self) -> Result<serde_json::Value, LogError> {
        let val = match self {
            PropertyValue::Set(v) => serde_json::to_value(v),
            PropertyValue::Struct(v) => serde_json::to_value(v),
            PropertyValue::Range(v) => serde_json::to_value(v),
        };
        val.map_err(|e| LogError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn set_roundtrip() {
        let val = PropertyValue::Set(SetValue {
            contains: vec![json!("gold"), json!("platinum")],
            not_contains: vec![json!("diamond")],
        });
        let bytes = val.to_bytes().unwrap();
        let restored = PropertyValue::from_bytes(&bytes, crate::schema::ValueType::Set).unwrap();
        assert_eq!(val, restored);
    }

    #[test]
    fn set_empty_fields_omitted() {
        let val = SetValue {
            contains: vec![json!("gold")],
            not_contains: vec![],
        };
        let json_str = serde_json::to_string(&val).unwrap();
        assert!(!json_str.contains("not_contains"));
    }

    #[test]
    fn range_eq_roundtrip() {
        let val = PropertyValue::Range(RangeValue::Eq(json!(120)));
        let bytes = val.to_bytes().unwrap();
        let restored = PropertyValue::from_bytes(&bytes, crate::schema::ValueType::Range).unwrap();
        assert_eq!(val, restored);
    }

    #[test]
    fn range_between_roundtrip() {
        let val = PropertyValue::Range(RangeValue::Between {
            from: json!(100),
            to: json!(200),
        });
        let bytes = val.to_bytes().unwrap();
        let restored = PropertyValue::from_bytes(&bytes, crate::schema::ValueType::Range).unwrap();
        assert_eq!(val, restored);
    }

    #[test]
    fn range_open_ended() {
        let from = PropertyValue::Range(RangeValue::From(json!(50)));
        let to = PropertyValue::Range(RangeValue::To(json!(999)));

        let from_bytes = from.to_bytes().unwrap();
        let to_bytes = to.to_bytes().unwrap();

        assert_eq!(from, PropertyValue::from_bytes(&from_bytes, crate::schema::ValueType::Range).unwrap());
        assert_eq!(to, PropertyValue::from_bytes(&to_bytes, crate::schema::ValueType::Range).unwrap());
    }

    #[test]
    fn struct_roundtrip() {
        let val = PropertyValue::Struct(StructValue(json!({"genre": "jazz", "bpm": 120})));
        let bytes = val.to_bytes().unwrap();
        let restored = PropertyValue::from_bytes(&bytes, crate::schema::ValueType::Struct).unwrap();
        assert_eq!(val, restored);
    }

    #[test]
    fn to_json_preserves_structure() {
        let set = PropertyValue::Set(SetValue {
            contains: vec![json!("a")],
            not_contains: vec![json!("b")],
        });
        let j = set.to_json().unwrap();
        assert_eq!(j["contains"], json!(["a"]));
        assert_eq!(j["not_contains"], json!(["b"]));

        let range = PropertyValue::Range(RangeValue::Eq(json!(42)));
        let j = range.to_json().unwrap();
        assert_eq!(j["eq"], json!(42));
    }
}
