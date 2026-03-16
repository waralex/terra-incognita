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
use typed_builder::TypedBuilder;

use super::DataSchema;

/// Top-level project configuration.
#[derive(Debug, Clone, Deserialize, TypedBuilder)]
pub struct ProjectConfig {
    /// Path to the RocksDB data directory.
    pub data_dir: PathBuf,

    /// Path to the data schema YAML file.
    pub schema_path: PathBuf,

    /// Maximum branch nesting depth. Default: 8.
    #[serde(default = "default_max_branch_depth")]
    #[builder(default = 8)]
    pub max_branch_depth: usize,

    /// Path to the ONNX embedding model directory (containing model.onnx + tokenizer.json).
    #[serde(default)]
    #[builder(default)]
    pub model_path: Option<PathBuf>,
}

fn default_max_branch_depth() -> usize {
    8
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
    /// Relative paths resolve from CWD, not from the config file location.
    pub fn load(config_path: &Path) -> Result<Project, ProjectError> {
        let config = Self::from_file(config_path)?;
        let schema = DataSchema::from_file(&config.schema_path)?;
        Ok(Project { config, schema })
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use super::*;
    use std::fs;

    #[test]
    fn parse_project_config() {
        let config = ProjectConfig::from_yaml(indoc! {"
            data_dir: ./data
            schema_path: ./schema.yaml
        "}).unwrap();
        assert_eq!(config.data_dir, PathBuf::from("./data"));
        assert_eq!(config.schema_path, PathBuf::from("./schema.yaml"));
        assert_eq!(config.max_branch_depth, 8);
    }

    #[test]
    fn builder() {
        let config = ProjectConfig::builder()
            .data_dir("./data".into())
            .schema_path("./schema.yaml".into())
            .build();
        assert_eq!(config.data_dir, PathBuf::from("./data"));
        assert_eq!(config.max_branch_depth, 8);
    }

    #[test]
    fn builder_custom_depth() {
        let config = ProjectConfig::builder()
            .data_dir("./data".into())
            .schema_path("./schema.yaml".into())
            .max_branch_depth(4)
            .build();
        assert_eq!(config.max_branch_depth, 4);
    }

    #[test]
    fn load_project_from_files() {
        let dir = tempfile::tempdir().unwrap();

        let schema_path = dir.path().join("schema.yaml");
        fs::write(&schema_path, indoc! {"
            transaction_meta:
              reasoning:
                type: text
                required: true
            managed_types: {}
        "}).unwrap();

        // schema_path is absolute — resolves as-is regardless of CWD
        let config_yaml = format!(
            "data_dir: ./data\nschema_path: {}",
            schema_path.display()
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
