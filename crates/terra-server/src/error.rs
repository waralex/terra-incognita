use axum::http::StatusCode;

/// Maps a QueryError kind string to an HTTP status code.
pub fn error_kind_to_status(kind: &str) -> StatusCode {
    match kind {
        "parse_error" | "invalid_slug" | "reserved_property" | "conflicting_facts"
        | "assertion_error" | "empty_transaction" => StatusCode::BAD_REQUEST,
        "duplicate_entity_type" | "duplicate_property" | "entity_already_exists"
        | "entity_status_conflict" | "branch_already_exists" => StatusCode::CONFLICT,
        "entity_type_not_found" | "property_not_found" | "entity_not_found"
        | "branch_not_found" => StatusCode::NOT_FOUND,
        "max_depth_exceeded" => StatusCode::BAD_REQUEST,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
