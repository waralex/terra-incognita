mod config;
mod dispatch;
mod dto;
mod error;
mod format;

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::{routing::post, Router};
use terra_core_v0_2::config::ProjectConfig;
use terra_core_v0_2::embed::NoopEmbedder;
use terra_core_v0_2::Terra;
use tracing::info;

use crate::config::ServerConfig;
use crate::format::ContentFormat;

async fn handle_query(
    State(terra): State<Arc<Terra>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let format = ContentFormat::from_headers(&headers);
    dispatch::handle(&terra, &body, format)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let server_config = ServerConfig::load();
    info!("project config: {}", server_config.project_config_path.display());
    info!("port: {}", server_config.port);

    let project = ProjectConfig::load(&server_config.project_config_path)
        .expect("failed to load project config");

    // Relative paths resolve from CWD, not from the config file location.
    let data_dir = project.config.data_dir.clone();

    std::fs::create_dir_all(&data_dir).expect("failed to create data directory");

    let embedder: Arc<dyn terra_core_v0_2::embed::Embedder> = Arc::new(NoopEmbedder);

    let terra = Terra::open(
        &data_dir,
        Arc::new(project.config),
        Arc::new(project.schema),
        embedder,
    )
    .expect("failed to open Terra");

    info!("data dir: {}", data_dir.display());

    let state = Arc::new(terra);

    let app = Router::new()
        .route("/query", post(handle_query))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", server_config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    info!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.expect("server error");
}
