use serde::Deserialize;
use std::path::PathBuf;
use tracing::info;

const CONFIG_FILENAME: &str = "terra-incognita.yml";
const CONFIG_ENV: &str = "TERRA_INCOGNITA_CONFIG";
const DEFAULT_DATA_DIR: &str = ".terra-incognita";
const DEFAULT_PORT: u16 = 3000;

/// Server configuration loaded from `terra-incognita.yml`.
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_data_dir() -> PathBuf {
    PathBuf::from(DEFAULT_DATA_DIR)
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            data_dir: default_data_dir(),
        }
    }
}

impl Config {
    /// Loads config from file (current dir, env var, home dir) or falls back to defaults.
    pub fn load() -> Self {
        let candidates = config_candidates();

        for path in &candidates {
            if path.exists() {
                info!("loading config from {}", path.display());
                match std::fs::read_to_string(path) {
                    Ok(contents) => match serde_yaml::from_str::<Config>(&contents) {
                        Ok(cfg) => return cfg,
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

        info!("no config found, using defaults");
        Config::default()
    }

    /// Returns the path to the SQLite schema database.
    pub fn schema_db_path(&self) -> PathBuf {
        self.data_dir.join("schema.db")
    }

    /// Returns the path to the RocksDB assertions directory.
    pub fn assertions_db_path(&self) -> PathBuf {
        self.data_dir.join("assertions")
    }

    /// Creates the data directory if it does not exist.
    pub fn ensure_data_dir(&self) -> std::io::Result<()> {
        if !self.data_dir.exists() {
            info!("creating data directory: {}", self.data_dir.display());
            std::fs::create_dir_all(&self.data_dir)?;
        }
        Ok(())
    }
}

fn config_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. Current directory
    paths.push(PathBuf::from(CONFIG_FILENAME));

    // 2. Environment variable
    if let Ok(env_path) = std::env::var(CONFIG_ENV) {
        paths.push(PathBuf::from(env_path));
    }

    // 3. Home directory
    if let Some(home) = home_dir() {
        paths.push(home.join(DEFAULT_DATA_DIR).join(CONFIG_FILENAME));
    }

    paths
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
