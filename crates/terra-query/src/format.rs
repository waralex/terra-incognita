/// Supported serialization formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentFormat {
    Yaml,
    Json,
}

impl ContentFormat {
    /// Deserializes bytes into a typed value.
    pub fn deserialize<'de, T: serde::Deserialize<'de>>(&self, bytes: &'de [u8]) -> Result<T, String> {
        match self {
            ContentFormat::Yaml => serde_yaml::from_slice(bytes).map_err(|e| e.to_string()),
            ContentFormat::Json => serde_json::from_slice(bytes).map_err(|e| e.to_string()),
        }
    }

    /// Serializes a serde_json::Value to bytes in this format.
    pub fn serialize_value(&self, value: &serde_json::Value) -> Vec<u8> {
        match self {
            ContentFormat::Yaml => {
                serde_yaml::to_string(value)
                    .unwrap_or_else(|_| "error:\n  kind: internal\n  message: serialization failed\n".into())
                    .into_bytes()
            }
            ContentFormat::Json => {
                serde_json::to_vec(value)
                    .unwrap_or_else(|_| br#"{"error":{"kind":"internal","message":"serialization failed"}}"#.to_vec())
            }
        }
    }

    /// Returns the MIME content type string for this format.
    pub fn content_type_header(&self) -> &'static str {
        match self {
            ContentFormat::Yaml => "application/yaml",
            ContentFormat::Json => "application/json",
        }
    }
}
