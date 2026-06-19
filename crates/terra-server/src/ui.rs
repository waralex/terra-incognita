//! Read-only web viewer for the memory store, served at `GET /ui`.
//!
//! A single self-contained page (inline CSS/JS, no external assets) that
//! drives the existing `POST /query` API from the browser. It only issues
//! read commands, so it cannot mutate the store.

use axum::response::Html;

/// The viewer page, embedded into the binary at build time.
const PAGE: &str = include_str!("ui.html");

/// Serve the memory viewer page.
pub async fn serve_ui() -> Html<&'static str> {
    Html(PAGE)
}
