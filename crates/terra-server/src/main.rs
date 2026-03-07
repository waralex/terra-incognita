mod dispatch;
mod error;
mod query;
mod state;

use axum::{Router, routing::post};
use axum::body::Bytes;
use axum::extract::State;
use axum::response::IntoResponse;
use std::sync::{Arc, Mutex};
use terra_core::schema::SchemaRegistry;
use tracing::info;

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

    let registry = SchemaRegistry::open_in_memory().expect("failed to open schema registry");
    let state: AppState = Arc::new(Mutex::new(registry));

    let app = Router::new()
        .route("/query", post(handle_query))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind");

    info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.expect("server error");
}
