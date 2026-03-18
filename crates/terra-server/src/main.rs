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
use terra_core::config::ProjectConfig;
use terra_core::embed::Embedder;
use terra_core::embed::NoopEmbedder;
use terra_core::Terra;
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

    let data_dir = project.config.data_dir.clone();

    std::fs::create_dir_all(&data_dir).expect("failed to create data directory");

    let abs_data_dir = std::path::absolute(&data_dir).unwrap_or(data_dir.clone());
    info!("opening storage: {}", abs_data_dir.display());

    let embedder: Arc<dyn Embedder> = create_embedder(&server_config);

    let terra = Terra::open(
        &data_dir,
        Arc::new(project.config),
        Arc::new(project.schema),
        embedder,
    )
    .expect("failed to open Terra");

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

fn create_embedder(config: &ServerConfig) -> Arc<dyn Embedder> {
    #[cfg(feature = "onnx")]
    if let Some(ref dir) = config.embed_model_dir {
        info!("loading ONNX embedder from {}", dir.display());
        let embedder = terra_core::embed::OnnxEmbedder::from_dir(dir)
            .expect("failed to load ONNX embedder");
        info!("ONNX embedder ready ({}d)", embedder.dimensions());
        return Arc::new(embedder);
    }

    #[cfg(not(feature = "onnx"))]
    if config.embed_model_dir.is_some() {
        panic!("embed_model_dir is set but terra-server was built without the 'onnx' feature");
    }

    info!("embeddings disabled (no embed_model_dir)");
    Arc::new(NoopEmbedder)
}
