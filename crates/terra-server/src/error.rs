//! Error mapping — DbError to HTTP status codes.

use axum::http::StatusCode;
use terra_core::io::DbError;

/// Map a DbError to (HTTP status code, error kind string).
pub fn classify(err: &DbError) -> (StatusCode, &'static str) {
    match err {
        DbError::Validation(_) => (StatusCode::BAD_REQUEST, "validation_error"),
        DbError::Storage(msg) => {
            if msg.contains("already exists") {
                (StatusCode::CONFLICT, "conflict")
            } else if msg.contains("not found") {
                (StatusCode::NOT_FOUND, "not_found")
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, "storage_error")
            }
        }
    }
}
