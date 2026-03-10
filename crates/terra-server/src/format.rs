use axum::http::HeaderMap;
use terra_query::ContentFormat;

/// Determines the content format from request headers.
///
/// Checks Content-Type first, then Accept. Defaults to YAML for backwards compatibility.
pub fn content_format_from_headers(headers: &HeaderMap) -> ContentFormat {
    for header_name in ["content-type", "accept"] {
        if let Some(val) = headers.get(header_name).and_then(|v| v.to_str().ok()) {
            if val.contains("application/json") {
                return ContentFormat::Json;
            }
        }
    }
    ContentFormat::Yaml
}
