//! Content format detection and serialization helpers.

use axum::http::HeaderMap;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// Supported wire formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentFormat {
    Json,
    Yaml,
}

impl ContentFormat {
    /// Detect format from request headers (Content-Type, then Accept). Default: YAML.
    pub fn from_headers(headers: &HeaderMap) -> Self {
        for name in ["content-type", "accept"] {
            if let Some(val) = headers.get(name).and_then(|v| v.to_str().ok()) {
                if val.contains("application/json") {
                    return Self::Json;
                }
            }
        }
        Self::Yaml
    }

    /// MIME content-type string for response headers.
    pub fn content_type(self) -> &'static str {
        match self {
            Self::Json => "application/json",
            Self::Yaml => "application/x-yaml",
        }
    }

    /// Deserialize bytes into a typed value.
    pub fn deserialize<T: DeserializeOwned>(self, bytes: &[u8]) -> Result<T, String> {
        match self {
            Self::Json => serde_json::from_slice(bytes).map_err(|e| e.to_string()),
            Self::Yaml => serde_yaml::from_slice(bytes).map_err(|e| e.to_string()),
        }
    }

    /// Serialize a value to bytes.
    pub fn serialize<T: Serialize>(self, value: &T) -> Result<Vec<u8>, String> {
        match self {
            Self::Json => serde_json::to_vec(value).map_err(|e| e.to_string()),
            Self::Yaml => serde_yaml::to_string(value)
                .map(|s| s.into_bytes())
                .map_err(|e| e.to_string()),
        }
    }
}
