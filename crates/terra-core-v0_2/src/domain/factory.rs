//! Domain object factory — validates inputs against DataSchema.

use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::config::{DataSchema, FieldDef, FieldType, ManagedTypeDef};
use crate::io::Slug;
use crate::domain::managed::Managed;
use crate::domain::transaction::Transaction;

/// Validation errors for domain object creation.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("missing required field \"{field}\"")]
    MissingField { field: String },

    #[error("unexpected field \"{field}\"")]
    UnexpectedField { field: String },

    #[error("field \"{field}\": expected text, got {actual}")]
    TypeMismatch { field: String, actual: String },

    #[error("unknown managed type \"{type_name}\"")]
    UnknownManagedType { type_name: String },

    #[error("managed type \"{type_name}\": invalid state \"{state}\"")]
    InvalidState { type_name: String, state: String },

    #[error("managed type \"{type_name}\": state is required (has lifecycle)")]
    MissingState { type_name: String },
}

/// Factory for creating validated domain objects.
///
/// Holds a reference to DataSchema and validates all inputs before
/// constructing domain objects. Invalid objects cannot be created.
#[derive(Clone)]
pub struct DomainFactory {
    schema: Arc<DataSchema>,
}

impl DomainFactory {
    /// Create a new factory with the given schema.
    pub fn new(schema: Arc<DataSchema>) -> Self {
        Self { schema }
    }

    /// Create a validated transaction.
    pub fn transaction(&self, meta: Map<String, Value>) -> Result<Transaction, ValidationError> {
        validate_fields(&meta, &self.schema.transaction_meta)?;
        Ok(Transaction::new(meta))
    }

    /// Create a validated managed item.
    pub fn managed(
        &self,
        type_name: Slug,
        slug: Slug,
        state: Option<String>,
        fields: Map<String, Value>,
    ) -> Result<Managed, ValidationError> {
        let type_def = self.schema.managed_types.get(type_name.as_str())
            .ok_or_else(|| ValidationError::UnknownManagedType {
                type_name: type_name.to_string(),
            })?;

        validate_fields(&fields, &type_def.fields)?;
        validate_state(type_name.as_str(), &state, type_def)?;

        Ok(Managed::new(type_name, slug, state, fields))
    }

    /// Access the underlying schema.
    pub fn schema(&self) -> &DataSchema {
        &self.schema
    }
}

fn validate_fields(
    values: &Map<String, Value>,
    defs: &IndexMap<String, FieldDef>,
) -> Result<(), ValidationError> {
    for (name, def) in defs {
        match values.get(name.as_str()) {
            None if def.required => {
                return Err(ValidationError::MissingField { field: name.clone() });
            }
            Some(val) if def.field_type == FieldType::Text && !val.is_string() => {
                return Err(ValidationError::TypeMismatch {
                    field: name.clone(),
                    actual: value_type_name(val).into(),
                });
            }
            _ => {}
        }
    }

    for key in values.keys() {
        if !defs.contains_key(key) {
            return Err(ValidationError::UnexpectedField { field: key.clone() });
        }
    }

    Ok(())
}

fn validate_state(
    type_name: &str,
    state: &Option<String>,
    type_def: &ManagedTypeDef,
) -> Result<(), ValidationError> {
    match (&type_def.lifecycle, state) {
        (Some(_), None) => Err(ValidationError::MissingState {
            type_name: type_name.into(),
        }),
        (None, Some(s)) => Err(ValidationError::InvalidState {
            type_name: type_name.into(),
            state: s.clone(),
        }),
        _ => Ok(()),
    }
}

fn value_type_name(v: &Value) -> &str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use super::*;

    fn schema_with_tx_meta() -> Arc<DataSchema> {
        Arc::new(DataSchema::from_yaml(indoc! {"
            transaction_meta:
              reasoning:
                type: text
                required: true
              question:
                type: text
              answer:
                type: text
        "}).unwrap())
    }

    fn schema_with_managed() -> Arc<DataSchema> {
        Arc::new(DataSchema::from_yaml(indoc! {"
            managed_types:
              task:
                fields:
                  goal: { type: json, required: true }
                  notes: { type: json }
                lifecycle:
                  initial: open
                  visible: [open]
              note:
                fields:
                  content: { type: text, required: true }
        "}).unwrap())
    }

    #[test]
    fn valid_transaction() {
        let f = DomainFactory::new(schema_with_tx_meta());
        let mut meta = Map::new();
        meta.insert("reasoning".into(), Value::String("test".into()));

        let tx = f.transaction(meta).unwrap();
        assert_eq!(tx.meta["reasoning"], "test");
    }

    #[test]
    fn transaction_missing_required() {
        let f = DomainFactory::new(schema_with_tx_meta());
        let meta = Map::new();

        let err = f.transaction(meta).unwrap_err();
        assert!(matches!(err, ValidationError::MissingField { field } if field == "reasoning"));
    }

    #[test]
    fn transaction_unexpected_field() {
        let f = DomainFactory::new(schema_with_tx_meta());
        let mut meta = Map::new();
        meta.insert("reasoning".into(), Value::String("ok".into()));
        meta.insert("bogus".into(), Value::String("nope".into()));

        let err = f.transaction(meta).unwrap_err();
        assert!(matches!(err, ValidationError::UnexpectedField { field } if field == "bogus"));
    }

    #[test]
    fn transaction_type_mismatch() {
        let f = DomainFactory::new(schema_with_tx_meta());
        let mut meta = Map::new();
        meta.insert("reasoning".into(), serde_json::json!(42));

        let err = f.transaction(meta).unwrap_err();
        assert!(matches!(err, ValidationError::TypeMismatch { field, .. } if field == "reasoning"));
    }

    #[test]
    fn valid_managed_with_lifecycle() {
        let f = DomainFactory::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("do stuff"));

        let m = f.managed(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            Some("open".into()),
            fields,
        ).unwrap();

        assert_eq!(m.slug.as_str(), "task-1");
        assert_eq!(m.state.as_deref(), Some("open"));
    }

    #[test]
    fn managed_missing_state_with_lifecycle() {
        let f = DomainFactory::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("do stuff"));

        let err = f.managed(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            None,
            fields,
        ).unwrap_err();
        assert!(matches!(err, ValidationError::MissingState { .. }));
    }

    #[test]
    fn managed_state_without_lifecycle() {
        let f = DomainFactory::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("content".into(), Value::String("hello".into()));

        let err = f.managed(
            "note".parse().unwrap(),
            "note-1".parse().unwrap(),
            Some("active".into()),
            fields,
        ).unwrap_err();
        assert!(matches!(err, ValidationError::InvalidState { .. }));
    }

    #[test]
    fn managed_unknown_type() {
        let f = DomainFactory::new(schema_with_managed());

        let err = f.managed(
            "nonexistent".parse().unwrap(),
            "item-1".parse().unwrap(),
            None,
            Map::new(),
        ).unwrap_err();
        assert!(matches!(err, ValidationError::UnknownManagedType { .. }));
    }

    #[test]
    fn managed_without_lifecycle_no_state() {
        let f = DomainFactory::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("content".into(), Value::String("hello".into()));

        let m = f.managed(
            "note".parse().unwrap(),
            "note-1".parse().unwrap(),
            None,
            fields,
        ).unwrap();

        assert!(m.state.is_none());
    }

    #[test]
    fn empty_schema_accepts_empty_meta() {
        let schema = Arc::new(DataSchema::from_yaml("{}").unwrap());
        let f = DomainFactory::new(schema);

        let tx = f.transaction(Map::new()).unwrap();
        assert!(tx.meta.is_empty());
    }
}
