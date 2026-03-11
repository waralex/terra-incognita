mod config;
mod error;
mod format;
mod state;
mod web;

use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::{Router, routing::{get, post}};
use serde::Deserialize;
use std::sync::Arc;
use terra_core::assertion::{AssertionStore, MAIN_BRANCH};
use tracing::info;
use uuid::Uuid;

use crate::config::Config;
use crate::error::error_kind_to_status;
use crate::format::content_format_from_headers;
use crate::state::AppState;

/// Resolves branch slug to (branch_id, ancestry) for schema registry.
fn resolve_branch(slug: &str, store: &AssertionStore) -> (Uuid, Vec<(Uuid, Uuid)>) {
    if slug == "main" || slug.is_empty() {
        return (MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
    }
    let Ok(Some(record)) = store.branches().get_by_slug(slug) else {
        return (MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
    };
    (record.id, record.ancestry)
}

async fn handle_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let format = content_format_from_headers(&headers);
    let ct = format.content_type_header();
    let store = state.open_store();
    let registry = store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
    match terra_query::dispatch(&body, format, &registry, &store) {
        Ok(bytes) => (StatusCode::OK, [(CONTENT_TYPE, ct)], bytes).into_response(),
        Err(e) => {
            let status = error_kind_to_status(&e.kind);
            (status, [(CONTENT_TYPE, ct)], e.serialize(format)).into_response()
        }
    }
}

async fn handle_index() -> Html<&'static str> {
    Html(web::INDEX_HTML)
}

#[derive(Deserialize)]
struct StateParams {
    #[serde(default = "default_slug")]
    slug: String,
    at_tx: Option<String>,
}

fn default_slug() -> String {
    "main".into()
}

async fn handle_api_state(
    State(state): State<AppState>,
    Query(params): Query<StateParams>,
) -> Response {
    let store = state.open_store();
    let (branch_id, ancestry) = resolve_branch(&params.slug, &store);
    let registry = store.schema_registry(branch_id, ancestry);
    let mut json_body = serde_json::json!({
        "command": "branch.state",
        "slug": params.slug,
        "last_transactions": 100
    });
    if let Some(ref tx_id) = params.at_tx {
        json_body["transaction_id"] = serde_json::Value::String(tx_id.clone());
    }
    let body_bytes = serde_json::to_vec(&json_body).unwrap();
    match terra_query::dispatch(&body_bytes, terra_query::ContentFormat::Json, &registry, &store) {
        Ok(bytes) => (
            StatusCode::OK,
            [(CONTENT_TYPE, "application/json")],
            bytes,
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(CONTENT_TYPE, "application/json")],
            e.serialize(terra_query::ContentFormat::Json),
        ).into_response(),
    }
}

async fn handle_api_branches(State(state): State<AppState>) -> Response {
    let store = state.open_store();
    let registry = store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
    let json_body = serde_json::json!({ "command": "branch.list" });
    let body_bytes = serde_json::to_vec(&json_body).unwrap();
    match terra_query::dispatch(&body_bytes, terra_query::ContentFormat::Json, &registry, &store) {
        Ok(bytes) => (
            StatusCode::OK,
            [(CONTENT_TYPE, "application/json")],
            bytes,
        ).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            [(CONTENT_TYPE, "application/json")],
            e.serialize(terra_query::ContentFormat::Json),
        ).into_response(),
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = Config::load();
    info!("data_dir: {}", config.data_dir.display());
    info!("port: {}", config.port);

    config
        .ensure_data_dir()
        .expect("failed to create data directory");

    let assertions_path = config.assertions_db_path();
    // Verify DB is accessible at startup
    AssertionStore::open_read_only(&assertions_path)
        .expect("failed to open assertion store (read-only)");
    info!("assertions_db: {} (read-only, re-opened per request)", assertions_path.display());

    let state: AppState = Arc::new(crate::state::Inner {
        db_path: assertions_path,
    });

    let app = Router::new()
        .route("/", get(handle_index))
        .route("/api/state", get(handle_api_state))
        .route("/api/branches", get(handle_api_branches))
        .route("/query", post(handle_query))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.expect("server error");
}
