//! Domain validator — checks domain objects against DataSchema.
//!
//! Used by clients for early error feedback and by executor
//! as a mandatory check before writing to storage.

use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::config::{DataSchema, FieldDef, FieldType, ManagedTypeDef};
use crate::domain::entity::Entity;
use crate::domain::managed::Managed;
use crate::domain::transaction::Transaction;

/// Validation errors for domain objects.
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

    #[error("entity \"{slug}\": description is required")]
    MissingDescription { slug: String },

    #[error("invalid assertion status \"{status}\"")]
    InvalidStatus { status: String },

    #[error("assertion status \"{status}\" set but no assertion_statuses are configured")]
    StatusesNotConfigured { status: String },

    #[error("invalid regex pattern \"{pattern}\": {message}")]
    InvalidRegex { pattern: String, message: String },
}

/// Validates domain objects against DataSchema.
///
/// Does not create objects — only checks existing ones.
/// Clients can use this for early error feedback.
/// Executor uses the same validation before writing.
#[derive(Clone)]
pub struct DomainValidator {
    schema: Arc<DataSchema>,
}

impl DomainValidator {
    /// Create a new validator with the given schema.
    pub fn new(schema: Arc<DataSchema>) -> Self {
        Self { schema }
    }

    /// Validate transaction metadata.
    pub fn check_transaction(&self, tx: &Transaction) -> Result<(), ValidationError> {
        validate_fields(&tx.meta, &self.schema.transaction_meta, true)
    }

    /// Validate branch metadata.
    pub fn check_branch(&self, meta: &Map<String, Value>) -> Result<(), ValidationError> {
        validate_fields(meta, &self.schema.branch_meta, true)
    }

    /// Validate an entity for creation (description required).
    pub fn check_entity_create(&self, entity: &Entity) -> Result<(), ValidationError> {
        if entity.description.is_none() {
            return Err(ValidationError::MissingDescription {
                slug: entity.slug.to_string(),
            });
        }
        if !entity.properties.is_empty() {
            self.check_entity_change_meta(&entity.meta)?;
        }
        self.check_status(&entity.status)
    }

    /// Validate an entity for update (all fields optional).
    pub fn check_entity_update(&self, entity: &Entity) -> Result<(), ValidationError> {
        if !entity.properties.is_empty() {
            self.check_entity_change_meta(&entity.meta)?;
        }
        self.check_status(&entity.status)
    }

    /// Validate an assertion status against `assertion_statuses`.
    fn check_status(&self, status: &Option<String>) -> Result<(), ValidationError> {
        match (&self.schema.assertion_statuses, status) {
            (Some(s), Some(st)) if !s.contains(st) => {
                Err(ValidationError::InvalidStatus { status: st.clone() })
            }
            (None, Some(st)) => Err(ValidationError::StatusesNotConfigured { status: st.clone() }),
            _ => Ok(()),
        }
    }

    /// Validate entity change metadata against `DataSchema.entity_change_meta`.
    pub fn check_entity_change_meta(
        &self,
        meta: &Map<String, Value>,
    ) -> Result<(), ValidationError> {
        validate_fields(meta, &self.schema.entity_change_meta, true)
    }

    /// Validate a managed item for creation (required fields checked).
    pub fn check_managed_create(&self, managed: &Managed) -> Result<(), ValidationError> {
        let type_def = self.resolve_managed_type(managed)?;
        validate_fields(&managed.fields, &type_def.fields, true)?;
        validate_state(managed.type_name.as_str(), &managed.state, type_def)
    }

    /// Validate a managed item for update (required fields not checked).
    /// State=None is allowed on update (means "carry forward existing state").
    pub fn check_managed_update(&self, managed: &Managed) -> Result<(), ValidationError> {
        let type_def = self.resolve_managed_type(managed)?;
        validate_fields(&managed.fields, &type_def.fields, false)?;
        if managed.state.is_some() {
            validate_state(managed.type_name.as_str(), &managed.state, type_def)?;
        }
        Ok(())
    }

    /// Access the underlying schema.
    pub fn schema(&self) -> &DataSchema {
        &self.schema
    }

    fn resolve_managed_type(&self, managed: &Managed) -> Result<&ManagedTypeDef, ValidationError> {
        self.schema
            .managed_types
            .get(managed.type_name.as_str())
            .ok_or_else(|| ValidationError::UnknownManagedType {
                type_name: managed.type_name.to_string(),
            })
    }
}

/// Validate field values against definitions.
///
/// When `check_required` is true, missing required fields are errors (create).
/// When false, only type checks and unknown field checks apply (update).
fn validate_fields(
    values: &Map<String, Value>,
    defs: &IndexMap<String, FieldDef>,
    check_required: bool,
) -> Result<(), ValidationError> {
    for (name, def) in defs {
        match values.get(name.as_str()) {
            None if check_required && def.required => {
                return Err(ValidationError::MissingField {
                    field: name.clone(),
                });
            }
            Some(val)
                if def.field_type == FieldType::Text && !val.is_string() && !val.is_null() =>
            {
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
        (Some(lc), Some(s)) => {
            if !lc.states.contains(s) {
                Err(ValidationError::InvalidState {
                    type_name: type_name.into(),
                    state: s.clone(),
                })
            } else {
                Ok(())
            }
        }
        (None, Some(s)) => Err(ValidationError::InvalidState {
            type_name: type_name.into(),
            state: s.clone(),
        }),
        (None, None) => Ok(()),
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
    use super::*;
    use crate::domain::entity::Entity;
    use indoc::indoc;

    fn schema_with_tx_meta() -> Arc<DataSchema> {
        Arc::new(
            DataSchema::from_yaml(indoc! {"
            transaction_meta:
              reasoning:
                type: text
                required: true
              question:
                type: text
              answer:
                type: text
        "})
            .unwrap(),
        )
    }

    fn schema_with_managed() -> Arc<DataSchema> {
        Arc::new(
            DataSchema::from_yaml(indoc! {"
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
        "})
            .unwrap(),
        )
    }

    // --- Transaction ---

    #[test]
    fn valid_transaction() {
        let v = DomainValidator::new(schema_with_tx_meta());
        let mut meta = Map::new();
        meta.insert("reasoning".into(), Value::String("test".into()));

        v.check_transaction(&Transaction::new(meta)).unwrap();
    }

    #[test]
    fn transaction_missing_required() {
        let v = DomainValidator::new(schema_with_tx_meta());
        let err = v
            .check_transaction(&Transaction::new(Map::new()))
            .unwrap_err();
        assert!(matches!(err, ValidationError::MissingField { field } if field == "reasoning"));
    }

    #[test]
    fn transaction_unexpected_field() {
        let v = DomainValidator::new(schema_with_tx_meta());
        let mut meta = Map::new();
        meta.insert("reasoning".into(), Value::String("ok".into()));
        meta.insert("bogus".into(), Value::String("nope".into()));

        let err = v.check_transaction(&Transaction::new(meta)).unwrap_err();
        assert!(matches!(err, ValidationError::UnexpectedField { field } if field == "bogus"));
    }

    #[test]
    fn transaction_type_mismatch() {
        let v = DomainValidator::new(schema_with_tx_meta());
        let mut meta = Map::new();
        meta.insert("reasoning".into(), serde_json::json!(42));

        let err = v.check_transaction(&Transaction::new(meta)).unwrap_err();
        assert!(matches!(err, ValidationError::TypeMismatch { field, .. } if field == "reasoning"));
    }

    #[test]
    fn empty_schema_accepts_empty_meta() {
        let v = DomainValidator::new(Arc::new(DataSchema::from_yaml("{}").unwrap()));
        v.check_transaction(&Transaction::new(Map::new())).unwrap();
    }

    // --- Entity ---

    #[test]
    fn entity_create_with_description() {
        let v = DomainValidator::new(schema_with_tx_meta());
        let e = Entity::new(
            "person".parse().unwrap(),
            Some(serde_json::json!("A person")),
            vec![],
            Map::new(),
        );
        v.check_entity_create(&e).unwrap();
    }

    #[test]
    fn entity_create_missing_description() {
        let v = DomainValidator::new(schema_with_tx_meta());
        let e = Entity::new("person".parse().unwrap(), None, vec![], Map::new());

        let err = v.check_entity_create(&e).unwrap_err();
        assert!(matches!(err, ValidationError::MissingDescription { slug } if slug == "person"));
    }

    #[test]
    fn entity_update_without_description() {
        let v = DomainValidator::new(schema_with_tx_meta());
        let e = Entity::new("person".parse().unwrap(), None, vec![], Map::new());
        v.check_entity_update(&e).unwrap();
    }

    // --- Managed create ---

    #[test]
    fn create_managed_with_lifecycle() {
        let v = DomainValidator::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("do stuff"));

        let m = Managed::new(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            Some("open".into()),
            fields,
        );
        v.check_managed_create(&m).unwrap();
    }

    #[test]
    fn create_managed_missing_required() {
        let v = DomainValidator::new(schema_with_managed());
        let m = Managed::new(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            Some("open".into()),
            Map::new(),
        );

        let err = v.check_managed_create(&m).unwrap_err();
        assert!(matches!(err, ValidationError::MissingField { field } if field == "goal"));
    }

    #[test]
    fn create_managed_missing_state_with_lifecycle() {
        let v = DomainValidator::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("do stuff"));

        let m = Managed::new(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            None,
            fields,
        );
        let err = v.check_managed_create(&m).unwrap_err();
        assert!(matches!(err, ValidationError::MissingState { .. }));
    }

    #[test]
    fn create_managed_state_without_lifecycle() {
        let v = DomainValidator::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("content".into(), Value::String("hello".into()));

        let m = Managed::new(
            "note".parse().unwrap(),
            "note-1".parse().unwrap(),
            Some("active".into()),
            fields,
        );
        let err = v.check_managed_create(&m).unwrap_err();
        assert!(matches!(err, ValidationError::InvalidState { .. }));
    }

    #[test]
    fn create_managed_unknown_type() {
        let v = DomainValidator::new(schema_with_managed());
        let m = Managed::new(
            "nonexistent".parse().unwrap(),
            "item-1".parse().unwrap(),
            None,
            Map::new(),
        );

        let err = v.check_managed_create(&m).unwrap_err();
        assert!(matches!(err, ValidationError::UnknownManagedType { .. }));
    }

    #[test]
    fn create_managed_without_lifecycle() {
        let v = DomainValidator::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("content".into(), Value::String("hello".into()));

        let m = Managed::new(
            "note".parse().unwrap(),
            "note-1".parse().unwrap(),
            None,
            fields,
        );
        v.check_managed_create(&m).unwrap();
    }

    // --- Managed update ---

    #[test]
    fn update_managed_skips_required() {
        let v = DomainValidator::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("notes".into(), serde_json::json!("just notes"));

        let m = Managed::new(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            Some("open".into()),
            fields,
        );
        v.check_managed_update(&m).unwrap();
    }

    #[test]
    fn update_managed_still_checks_types() {
        let v = DomainValidator::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("content".into(), serde_json::json!(42));

        let m = Managed::new(
            "note".parse().unwrap(),
            "note-1".parse().unwrap(),
            None,
            fields,
        );
        let err = v.check_managed_update(&m).unwrap_err();
        assert!(matches!(err, ValidationError::TypeMismatch { field, .. } if field == "content"));
    }

    #[test]
    fn update_managed_still_checks_unknown() {
        let v = DomainValidator::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("bogus".into(), serde_json::json!("nope"));

        let m = Managed::new(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            Some("open".into()),
            fields,
        );
        let err = v.check_managed_update(&m).unwrap_err();
        assert!(matches!(err, ValidationError::UnexpectedField { field } if field == "bogus"));
    }

    // --- Entity change meta ---

    fn schema_with_entity_change_meta() -> Arc<DataSchema> {
        Arc::new(
            DataSchema::from_yaml(indoc! {"
            entity_change_meta:
              reasoning:
                type: text
                required: true
              confidence:
                type: json
        "})
            .unwrap(),
        )
    }

    #[test]
    fn entity_create_with_properties_validates_change_meta() {
        let v = DomainValidator::new(schema_with_entity_change_meta());
        use crate::domain::entity::PropertyValue;

        let mut meta = Map::new();
        meta.insert("reasoning".into(), Value::String("observed".into()));

        let e = Entity::new(
            "person".parse().unwrap(),
            Some(serde_json::json!("A person")),
            vec![PropertyValue {
                property: "age".parse().unwrap(),
                value: serde_json::json!(30),
                context: (),
            }],
            meta,
        );
        v.check_entity_create(&e).unwrap();
    }

    #[test]
    fn entity_create_with_properties_missing_required_meta() {
        let v = DomainValidator::new(schema_with_entity_change_meta());
        use crate::domain::entity::PropertyValue;

        let e = Entity::new(
            "person".parse().unwrap(),
            Some(serde_json::json!("A person")),
            vec![PropertyValue {
                property: "age".parse().unwrap(),
                value: serde_json::json!(30),
                context: (),
            }],
            Map::new(),
        );
        let err = v.check_entity_create(&e).unwrap_err();
        assert!(matches!(err, ValidationError::MissingField { field } if field == "reasoning"));
    }

    #[test]
    fn entity_create_without_properties_skips_change_meta() {
        let v = DomainValidator::new(schema_with_entity_change_meta());
        let e = Entity::new(
            "person".parse().unwrap(),
            Some(serde_json::json!("A person")),
            vec![],
            Map::new(),
        );
        v.check_entity_create(&e).unwrap();
    }

    #[test]
    fn entity_update_with_properties_validates_change_meta() {
        let v = DomainValidator::new(schema_with_entity_change_meta());
        use crate::domain::entity::PropertyValue;

        let e = Entity::new(
            "person".parse().unwrap(),
            None,
            vec![PropertyValue {
                property: "age".parse().unwrap(),
                value: serde_json::json!(31),
                context: (),
            }],
            Map::new(),
        );
        let err = v.check_entity_update(&e).unwrap_err();
        assert!(matches!(err, ValidationError::MissingField { field } if field == "reasoning"));
    }

    #[test]
    fn entity_update_without_properties_skips_change_meta() {
        let v = DomainValidator::new(schema_with_entity_change_meta());
        let e = Entity::new("person".parse().unwrap(), None, vec![], Map::new());
        v.check_entity_update(&e).unwrap();
    }

    // --- Lifecycle state validation ---

    #[test]
    fn create_managed_with_invalid_state() {
        let v = DomainValidator::new(schema_with_managed());
        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("do stuff"));

        let m = Managed::new(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            Some("banana".into()),
            fields,
        );
        let err = v.check_managed_create(&m).unwrap_err();
        assert!(
            matches!(err, ValidationError::InvalidState { type_name, state }
            if type_name == "task" && state == "banana")
        );
    }

    // --- Assertion status validation ---

    fn schema_with_statuses() -> Arc<DataSchema> {
        Arc::new(
            DataSchema::from_yaml(indoc! {"
            entity_change_meta:
              reasoning: { type: text, required: true }
            assertion_statuses:
              values: [fact, hypothesis]
              terminal: fact
              default: hypothesis
        "})
            .unwrap(),
        )
    }

    #[test]
    fn entity_valid_status_accepted() {
        let v = DomainValidator::new(schema_with_statuses());
        let e = Entity::new(
            "alice".parse().unwrap(),
            Some(serde_json::json!("A person")),
            vec![],
            Map::new(),
        )
        .with_status(Some("fact".into()));
        v.check_entity_create(&e).unwrap();
    }

    #[test]
    fn entity_invalid_status_rejected() {
        let v = DomainValidator::new(schema_with_statuses());
        let e = Entity::new(
            "alice".parse().unwrap(),
            Some(serde_json::json!("A person")),
            vec![],
            Map::new(),
        )
        .with_status(Some("guess".into()));
        let err = v.check_entity_create(&e).unwrap_err();
        assert!(matches!(err, ValidationError::InvalidStatus { status } if status == "guess"));
    }

    #[test]
    fn entity_status_without_config_rejected() {
        let v = DomainValidator::new(schema_with_tx_meta());
        let e = Entity::new("alice".parse().unwrap(), None, vec![], Map::new())
            .with_status(Some("fact".into()));
        let err = v.check_entity_update(&e).unwrap_err();
        assert!(matches!(err, ValidationError::StatusesNotConfigured { .. }));
    }

    #[test]
    fn entity_no_status_is_fine() {
        let v = DomainValidator::new(schema_with_statuses());
        let e = Entity::new(
            "alice".parse().unwrap(),
            Some(serde_json::json!("A person")),
            vec![],
            Map::new(),
        );
        v.check_entity_create(&e).unwrap();
    }

    #[test]
    fn update_managed_with_invalid_state() {
        let v = DomainValidator::new(schema_with_managed());

        let m = Managed::new(
            "task".parse().unwrap(),
            "task-1".parse().unwrap(),
            Some("banana".into()),
            Map::new(),
        );
        let err = v.check_managed_update(&m).unwrap_err();
        assert!(
            matches!(err, ValidationError::InvalidState { type_name, state }
            if type_name == "task" && state == "banana")
        );
    }
}
