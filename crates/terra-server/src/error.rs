use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use terra_core::assertion::AssertionError;
use terra_core::command::CommandError;
use terra_core::schema::SchemaError;

pub struct ApiError {
    pub status: StatusCode,
    pub kind: String,
    pub message: String,
}

impl ApiError {
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

impl From<AssertionError> for ApiError {
    fn from(err: AssertionError) -> Self {
        let (status, kind) = match &err {
            AssertionError::InvalidName(_) => (StatusCode::BAD_REQUEST, "invalid_name"),
            AssertionError::EntityTypeNotFound(_) => (StatusCode::NOT_FOUND, "entity_type_not_found"),
            AssertionError::BatchItemError { source, .. } => {
                let (inner_status, inner_kind) = match source.as_ref() {
                    AssertionError::InvalidName(_) => (StatusCode::BAD_REQUEST, "invalid_name"),
                    AssertionError::EntityTypeNotFound(_) => (StatusCode::NOT_FOUND, "entity_type_not_found"),
                    _ => (StatusCode::INTERNAL_SERVER_ERROR, "storage_error"),
                };
                return Self {
                    status: inner_status,
                    kind: inner_kind.to_string(),
                    message: err.to_string(),
                };
            }
            AssertionError::Storage(_) => (StatusCode::INTERNAL_SERVER_ERROR, "storage_error"),
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
            CommandError::Assertion(e) => e.into(),
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
