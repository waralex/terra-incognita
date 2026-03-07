mod config;
mod dispatch;
mod error;
mod query;
mod state;

use axum::body::Bytes;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::{Router, routing::post};
use std::sync::{Arc, Mutex};
use terra_core::assertion::AssertionStore;
use terra_core::schema::SchemaRegistry;
use tracing::info;

use crate::config::Config;
use crate::dispatch::dispatch;
use crate::query::Command;
use crate::state::AppState;

async fn handle_query(State(state): State<AppState>, body: Bytes) -> impl IntoResponse {
    let cmd = match Command::parse(&body) {
        Ok(cmd) => cmd,
        Err(e) => return e.into_response(),
    };

    match dispatch(cmd, &state) {
        Ok(val) => {
            let yaml = serde_yaml::to_string(&val).unwrap();
            (
                axum::http::StatusCode::OK,
                [("content-type", "application/yaml")],
                yaml,
            )
                .into_response()
        }
        Err(e) => e.into_response(),
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
    let assertions = AssertionStore::open(&assertions_path).expect("failed to open assertion store");
    info!("assertions_db: {}", assertions_path.display());

    let state: AppState = Arc::new(Mutex::new(crate::state::Inner { registry, assertions }));

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
