//! Server configuration — port, project config path.

use std::path::PathBuf;

use serde::Deserialize;
use tracing::info;

const CONFIG_FILENAME: &str = "terra-server.yaml";
const CONFIG_ENV: &str = "TERRA_SERVER_CONFIG";
const DEFAULT_PORT: u16 = 3000;

/// Server configuration loaded from `terra-server.yaml`.
#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    pub project_config_path: PathBuf,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl ServerConfig {
    /// Load config from file (cwd, env var, home dir) or panic with a helpful message.
    pub fn load() -> Self {
        let candidates = config_candidates();

        for path in &candidates {
            if path.exists() {
                info!("loading config from {}", path.display());
                match std::fs::read_to_string(path) {
                    Ok(contents) => match serde_yaml::from_str::<ServerConfig>(&contents) {
                        Ok(mut cfg) => {
                            // Resolve project_config_path relative to the server config file.
                            if cfg.project_config_path.is_relative() {
                                let base = path.parent().unwrap_or(std::path::Path::new("."));
                                cfg.project_config_path = base.join(&cfg.project_config_path);
                            }
                            return cfg;
                        }
                        Err(e) => {
                            eprintln!("warning: failed to parse {}: {e}", path.display());
                        }
                    },
                    Err(e) => {
                        eprintln!("warning: failed to read {}: {e}", path.display());
                    }
                }
            }
        }

        panic!(
            "no server config found. Create {} with at least `project_config_path`.",
            CONFIG_FILENAME
        );
    }
}

fn config_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(PathBuf::from(CONFIG_FILENAME));
    paths.push(PathBuf::from(".terra-incognita").join(CONFIG_FILENAME));
    if let Ok(env_path) = std::env::var(CONFIG_ENV) {
        paths.push(PathBuf::from(env_path));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        paths.push(home.join(".terra-incognita").join(CONFIG_FILENAME));
    }
    paths
}
