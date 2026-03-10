use terra_core::assertion::EntityError;
use terra_core::command::{
    AssertEntityError, CommandError, ProjectionError, SessionCommandError,
};
use terra_core::schema::SchemaError;

use crate::format::ContentFormat;

/// Transport-agnostic query error with kind tag and human-readable message.
pub struct QueryError {
    pub kind: String,
    pub message: String,
}

impl QueryError {
    /// Creates a parse/validation error.
    pub fn bad_request(kind: &str, message: impl Into<String>) -> Self {
        Self {
            kind: kind.to_string(),
            message: message.into(),
        }
    }

    /// Serializes the error as `{error: {kind, message}}` in the given format.
    pub fn serialize(&self, format: ContentFormat) -> Vec<u8> {
        let value = serde_json::json!({
            "error": {
                "kind": self.kind,
                "message": self.message,
            }
        });
        format.serialize_value(&value)
    }
}

impl From<SchemaError> for QueryError {
    fn from(err: SchemaError) -> Self {
        let kind = match &err {
            SchemaError::InvalidSlug(_) => "invalid_slug",
            SchemaError::DuplicateEntityType(_) => "duplicate_entity_type",
            SchemaError::DuplicateProperty(_) => "duplicate_property",
            SchemaError::EntityTypeNotFound(_) => "entity_type_not_found",
            SchemaError::PropertyNotFound(_) => "property_not_found",
            SchemaError::ReservedProperty(_) => "reserved_property",
            SchemaError::BatchItemError { source, .. } => {
                let inner_kind = match source.as_ref() {
                    SchemaError::InvalidSlug(_) => "invalid_slug",
                    SchemaError::DuplicateEntityType(_) => "duplicate_entity_type",
                    SchemaError::DuplicateProperty(_) => "duplicate_property",
                    SchemaError::EntityTypeNotFound(_) => "entity_type_not_found",
                    SchemaError::PropertyNotFound(_) => "property_not_found",
                    SchemaError::ReservedProperty(_) => "reserved_property",
                    _ => "database_error",
                };
                return Self {
                    kind: inner_kind.to_string(),
                    message: err.to_string(),
                };
            }
            SchemaError::Db(_) => "database_error",
        };
        Self {
            kind: kind.to_string(),
            message: err.to_string(),
        }
    }
}

impl From<CommandError> for QueryError {
    fn from(err: CommandError) -> Self {
        match err {
            CommandError::Schema(e) => e.into(),
            CommandError::Log(e) => Self {
                kind: "storage_error".to_string(),
                message: e.to_string(),
            },
            CommandError::Entity(e) => {
                let kind = match &e {
                    EntityError::SlugExists(_) => "entity_already_exists",
                    EntityError::NotFound(_) | EntityError::SlugNotFound(_) => "entity_not_found",
                    EntityError::AlreadyInStatus(_, _) => "entity_status_conflict",
                    EntityError::Storage(_) => "storage_error",
                };
                Self {
                    kind: kind.to_string(),
                    message: e.to_string(),
                }
            }
            CommandError::Writer(e) => Self {
                kind: "assertion_error".to_string(),
                message: e.to_string(),
            },
            CommandError::Projection(e) => {
                let kind = match &e {
                    ProjectionError::EntityNotFound(_) => "entity_not_found",
                    ProjectionError::Entity(_) | ProjectionError::Storage(_) => "storage_error",
                    ProjectionError::Schema(_) => "entity_type_not_found",
                };
                Self {
                    kind: kind.to_string(),
                    message: e.to_string(),
                }
            }
            CommandError::Session(e) => {
                use terra_core::assertion::SessionError;
                let kind = match &e {
                    SessionCommandError::SessionNotFound(_) => "session_not_found",
                    SessionCommandError::EntityTypeNotFound(_) => "entity_type_not_found",
                    SessionCommandError::EntityNotFound(_) => "entity_not_found",
                    SessionCommandError::Session(se) => match se {
                        SessionError::SlugExists(_) => "session_already_exists",
                        SessionError::SlugNotFound(_) | SessionError::NotFound(_) => {
                            "session_not_found"
                        }
                        SessionError::Storage(_) => "storage_error",
                    },
                    SessionCommandError::Schema(_) | SessionCommandError::Entity(_) => {
                        "internal_error"
                    }
                };
                Self {
                    kind: kind.to_string(),
                    message: e.to_string(),
                }
            }
            CommandError::AssertEntity(e) => {
                let kind = match &e {
                    AssertEntityError::EntityNotFound(_) => "entity_not_found",
                    AssertEntityError::EntityAlreadyExists(_) => "entity_already_exists",
                    AssertEntityError::ConflictingFacts { .. } => "conflicting_facts",
                    AssertEntityError::EntityTypeNotFound(_) => "entity_type_not_found",
                    AssertEntityError::PropertyNotFound { .. } => "property_not_found",
                    AssertEntityError::Entity(_)
                    | AssertEntityError::Writer(_)
                    | AssertEntityError::Schema(_) => "internal_error",
                };
                Self {
                    kind: kind.to_string(),
                    message: e.to_string(),
                }
            }
        }
    }
}
