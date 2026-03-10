mod config;
mod error;
mod format;
mod state;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Router, routing::post};
use std::sync::{Arc, Mutex};
use terra_core::assertion::AssertionStore;
use terra_core::schema::SchemaRegistry;
use tracing::info;

use crate::config::Config;
use crate::error::error_kind_to_status;
use crate::format::content_format_from_headers;
use crate::state::AppState;

async fn handle_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let format = content_format_from_headers(&headers);
    let ct = format.content_type_header();
    let mut inner = state.lock().unwrap();
    let crate::state::Inner {
        ref mut registry,
        ref assertions,
    } = *inner;
    match terra_query::dispatch(&body, format, registry, assertions) {
        Ok(bytes) => (StatusCode::OK, [(CONTENT_TYPE, ct)], bytes).into_response(),
        Err(e) => {
            let status = error_kind_to_status(&e.kind);
            (status, [(CONTENT_TYPE, ct)], e.serialize(format)).into_response()
        }
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

    let db_path = config.schema_db_path();
    let registry = SchemaRegistry::open(&db_path).expect("failed to open schema registry");

    let assertions_path = config.assertions_db_path();
    let assertions =
        AssertionStore::open(&assertions_path).expect("failed to open assertion store");
    info!("assertions_db: {}", assertions_path.display());

    let state: AppState = Arc::new(Mutex::new(crate::state::Inner {
        registry,
        assertions,
    }));

    let app = Router::new()
        .route("/query", post(handle_query))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.expect("server error");
}
