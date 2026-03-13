//! Project configuration: paths and settings for a terra project.
//!
//! # Example
//!
//! ```yaml
//! data_dir: ./data
//! schema_path: ./schema.yaml
//! ```

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::DataSchema;

/// Top-level project configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ProjectConfig {
    /// Path to the RocksDB data directory.
    pub data_dir: PathBuf,

    /// Path to the data schema YAML file.
    pub schema_path: PathBuf,
}

/// A resolved project: config + parsed data schema, ready to use.
#[derive(Debug, Clone)]
pub struct Project {
    pub config: ProjectConfig,
    pub schema: DataSchema,
}

/// Errors from project loading.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("failed to read project config: {0}")]
    Io(#[from] std::io::Error),

    #[error("failed to parse project config: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("failed to load data schema: {0}")]
    Schema(#[from] super::ConfigError),
}

impl ProjectConfig {
    /// Parse project config from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, ProjectError> {
        let config: Self = serde_yaml::from_str(yaml)?;
        Ok(config)
    }

    /// Parse project config from a YAML file.
    pub fn from_file(path: &Path) -> Result<Self, ProjectError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }

    /// Load project config and resolve the data schema.
    /// Schema path is resolved relative to the config file's directory.
    pub fn load(config_path: &Path) -> Result<Project, ProjectError> {
        let config = Self::from_file(config_path)?;

        let base_dir = config_path.parent().unwrap_or(Path::new("."));
        let schema_path = if config.schema_path.is_relative() {
            base_dir.join(&config.schema_path)
        } else {
            config.schema_path.clone()
        };

        let schema = DataSchema::from_file(&schema_path)?;
        Ok(Project { config, schema })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_project_config() {
        let yaml = r#"
data_dir: ./data
schema_path: ./schema.yaml
"#;
        let config = ProjectConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.data_dir, PathBuf::from("./data"));
        assert_eq!(config.schema_path, PathBuf::from("./schema.yaml"));
    }

    #[test]
    fn load_project_from_files() {
        let dir = tempfile::tempdir().unwrap();

        let schema_yaml = r#"
transaction_meta:
  reasoning:
    type: text
    required: true
managed_types: {}
"#;
        let schema_path = dir.path().join("schema.yaml");
        fs::write(&schema_path, schema_yaml).unwrap();

        let config_yaml = format!(
            "data_dir: ./data\nschema_path: {}",
            schema_path.file_name().unwrap().to_str().unwrap()
        );
        let config_path = dir.path().join("terra.yaml");
        fs::write(&config_path, config_yaml).unwrap();

        let project = ProjectConfig::load(&config_path).unwrap();
        assert_eq!(project.config.data_dir, PathBuf::from("./data"));
        assert!(project.schema.transaction_meta.contains_key("reasoning"));
    }

    #[test]
    fn missing_schema_file_errors() {
        let dir = tempfile::tempdir().unwrap();

        let config_yaml = "data_dir: ./data\nschema_path: nonexistent.yaml";
        let config_path = dir.path().join("terra.yaml");
        fs::write(&config_path, config_yaml).unwrap();

        let err = ProjectConfig::load(&config_path).unwrap_err();
        assert!(matches!(err, ProjectError::Schema(_)));
    }
}
