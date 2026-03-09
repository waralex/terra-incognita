use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use terra_core::command::CommandError;
use terra_core::schema::SchemaError;

/// HTTP API error with status code, error kind, and human-readable message.
pub struct ApiError {
    pub status: StatusCode,
    pub kind: String,
    pub message: String,
}

impl ApiError {
    /// Creates a 400 Bad Request error with the given kind and message.
    pub fn bad_request(kind: &str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            kind: kind.to_string(),
            message: message.into(),
        }
    }
}

impl From<SchemaError> for ApiError {
    fn from(err: SchemaError) -> Self {
        let (status, kind) = match &err {
            SchemaError::InvalidSlug(_) => (StatusCode::BAD_REQUEST, "invalid_slug"),
            SchemaError::DuplicateEntityType(_) => (StatusCode::CONFLICT, "duplicate_entity_type"),
            SchemaError::DuplicateProperty(_) => (StatusCode::CONFLICT, "duplicate_property"),
            SchemaError::EntityTypeNotFound(_) => (StatusCode::NOT_FOUND, "entity_type_not_found"),
            SchemaError::PropertyNotFound(_) => (StatusCode::NOT_FOUND, "property_not_found"),
            SchemaError::ReservedProperty(_) => (StatusCode::BAD_REQUEST, "reserved_property"),
            SchemaError::BatchItemError { source, .. } => {
                let (inner_status, inner_kind) = match source.as_ref() {
                    SchemaError::InvalidSlug(_) => (StatusCode::BAD_REQUEST, "invalid_slug"),
                    SchemaError::DuplicateEntityType(_) => (StatusCode::CONFLICT, "duplicate_entity_type"),
                    SchemaError::DuplicateProperty(_) => (StatusCode::CONFLICT, "duplicate_property"),
                    SchemaError::EntityTypeNotFound(_) => (StatusCode::NOT_FOUND, "entity_type_not_found"),
                    SchemaError::PropertyNotFound(_) => (StatusCode::NOT_FOUND, "property_not_found"),
                    SchemaError::ReservedProperty(_) => (StatusCode::BAD_REQUEST, "reserved_property"),
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "database_error"),
                };
                return Self {
                    status: inner_status,
                    kind: inner_kind.to_string(),
                    message: err.to_string(),
                };
            }
            SchemaError::Db(_) => (StatusCode::INTERNAL_SERVER_ERROR, "database_error"),
        };
        Self {
            status,
            kind: kind.to_string(),
            message: err.to_string(),
        }
    }
}

impl From<CommandError> for ApiError {
    fn from(err: CommandError) -> Self {
        match err {
            CommandError::Schema(e) => e.into(),
            CommandError::Log(e) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                kind: "storage_error".to_string(),
                message: e.to_string(),
            },
            CommandError::Entity(e) => {
                use terra_core::assertion::EntityError;
                let (status, kind) = match &e {
                    EntityError::SlugExists(_) => (StatusCode::CONFLICT, "entity_already_exists"),
                    EntityError::NotFound(_) | EntityError::SlugNotFound(_) => {
                        (StatusCode::NOT_FOUND, "entity_not_found")
                    }
                    EntityError::AlreadyInStatus(_, _) => {
                        (StatusCode::CONFLICT, "entity_status_conflict")
                    }
                    EntityError::Storage(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, "storage_error")
                    }
                };
                Self {
                    status,
                    kind: kind.to_string(),
                    message: e.to_string(),
                }
            }
            CommandError::Writer(e) => Self {
                status: StatusCode::BAD_REQUEST,
                kind: "assertion_error".to_string(),
                message: e.to_string(),
            },
            CommandError::Projection(e) => {
                use terra_core::command::ProjectionError;
                let (status, kind) = match &e {
                    ProjectionError::EntityNotFound(_) => {
                        (StatusCode::NOT_FOUND, "entity_not_found")
                    }
                    ProjectionError::Entity(_) | ProjectionError::Storage(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, "storage_error")
                    }
                    ProjectionError::Schema(_) => {
                        (StatusCode::NOT_FOUND, "entity_type_not_found")
                    }
                };
                Self {
                    status,
                    kind: kind.to_string(),
                    message: e.to_string(),
                }
            }
            CommandError::AssertEntity(e) => {
                use terra_core::command::AssertEntityError;
                let (status, kind) = match &e {
                    AssertEntityError::EntityNotFound(_) => {
                        (StatusCode::NOT_FOUND, "entity_not_found")
                    }
                    AssertEntityError::EntityAlreadyExists(_) => {
                        (StatusCode::CONFLICT, "entity_already_exists")
                    }
                    AssertEntityError::ConflictingFacts { .. } => {
                        (StatusCode::BAD_REQUEST, "conflicting_facts")
                    }
                    AssertEntityError::EntityTypeNotFound(_) => {
                        (StatusCode::NOT_FOUND, "entity_type_not_found")
                    }
                    AssertEntityError::PropertyNotFound { .. } => {
                        (StatusCode::NOT_FOUND, "property_not_found")
                    }
                    AssertEntityError::Entity(_)
                    | AssertEntityError::Writer(_)
                    | AssertEntityError::Schema(_) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
                    }
                };
                Self {
                    status,
                    kind: kind.to_string(),
                    message: e.to_string(),
                }
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_yaml::to_string(&serde_yaml::Value::Mapping({
            let mut error = serde_yaml::Mapping::new();
            error.insert(
                serde_yaml::Value::String("kind".into()),
                serde_yaml::Value::String(self.kind),
            );
            error.insert(
                serde_yaml::Value::String("message".into()),
                serde_yaml::Value::String(self.message),
            );
            let mut root = serde_yaml::Mapping::new();
            root.insert(
                serde_yaml::Value::String("error".into()),
                serde_yaml::Value::Mapping(error),
            );
            root
        }))
        .unwrap_or_else(|_| "error:\n  kind: internal\n  message: serialization failed\n".into());

        (
            self.status,
            [("content-type", "application/yaml")],
            body,
        )
            .into_response()
    }
}
