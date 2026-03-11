use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use terra_core::assertion::{AssertionStore, MAIN_BRANCH};
use terra_query::ContentFormat;
use uuid::Uuid;

/// Thin wrapper around AssertionStore for dispatching YAML queries.
///
/// Cheaply cloneable via internal `Arc`.
#[derive(Clone)]
pub struct StoreHandle {
    store: Arc<AssertionStore>,
    log_path: Option<PathBuf>,
}

impl StoreHandle {
    /// Opens an AssertionStore at the given path.
    pub fn open(path: &Path) -> Self {
        let store = AssertionStore::open(path).expect("failed to open assertion store");
        Self { store: Arc::new(store), log_path: None }
    }

    /// Sets the query log file path.
    pub fn with_log(mut self, path: PathBuf) -> Self {
        self.log_path = Some(path);
        self
    }

    /// Dispatches a YAML command string and returns the YAML response.
    pub fn dispatch(&self, input: &str, branch: &str) -> Result<String, String> {
        self.log_query("QUERY", input);
        let (branch_id, ancestry) = self.resolve_branch(branch)?;
        let registry = self.store.schema_registry(branch_id, ancestry);
        let result = terra_query::dispatch(input.as_bytes(), ContentFormat::Yaml, &registry, &self.store);
        match &result {
            Ok(bytes) => {
                let yaml = String::from_utf8_lossy(bytes);
                self.log_query("RESULT", &yaml);
                Ok(yaml.into_owned())
            }
            Err(e) => {
                let err_bytes = e.serialize(ContentFormat::Yaml);
                let err = String::from_utf8_lossy(&err_bytes);
                self.log_query("ERROR", &err);
                Err(err.into_owned())
            }
        }
    }

    /// Fetches branch state as YAML for the side panel.
    pub fn fetch_state(&self, branch: &str) -> Result<String, String> {
        let cmd = format!("command: branch.state\nslug: {branch}");
        self.dispatch(&cmd, branch)
    }

    /// Lists all branch slugs (sessions).
    pub fn list_branches(&self) -> Result<Vec<String>, String> {
        let cmd = "command: branch.list";
        let registry = self.store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
        let bytes = terra_query::dispatch(cmd.as_bytes(), ContentFormat::Yaml, &registry, &self.store)
            .map_err(|e| String::from_utf8_lossy(&e.serialize(ContentFormat::Yaml)).into_owned())?;
        let val: serde_json::Value = serde_yaml::from_slice(&bytes).map_err(|e| e.to_string())?;
        let slugs = val.as_array()
            .map(|arr| arr.iter()
                .filter_map(|v| v["slug"].as_str().map(String::from))
                .collect())
            .unwrap_or_default();
        Ok(slugs)
    }

    /// Creates a new branch (session) from main.
    pub fn create_branch(&self, slug: &str, reasoning: &str) -> Result<(), String> {
        let cmd = format!(
            "command: branch.create\nslug: {slug}\nreasoning: {reasoning}\nparent: main"
        );
        let registry = self.store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
        terra_query::dispatch(cmd.as_bytes(), ContentFormat::Yaml, &registry, &self.store)
            .map_err(|e| String::from_utf8_lossy(&e.serialize(ContentFormat::Yaml)).into_owned())?;
        Ok(())
    }

    /// Returns last N (question, answer) pairs from the branch transaction history.
    pub fn fetch_history(&self, branch: &str, limit: usize) -> Vec<(String, String)> {
        let Ok((branch_id, _)) = self.resolve_branch(branch) else {
            return Vec::new();
        };
        let Ok(txs) = self.store.transactions().list_by_branch(&branch_id) else {
            return Vec::new();
        };
        txs.iter()
            .rev()
            .filter_map(|tx| {
                let q = tx.question.as_ref()?;
                let a = tx.answer.as_ref()?;
                if q.is_empty() && a.is_empty() {
                    return None;
                }
                Some((q.clone(), a.clone()))
            })
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    fn resolve_branch(&self, slug: &str) -> Result<(Uuid, Vec<(Uuid, Uuid)>), String> {
        if slug == "main" {
            return Ok((MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]));
        }
        // For non-main branches, look up via branch.get
        let cmd = format!("command: branch.get\nslug: {slug}");
        let registry = self.store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
        let bytes = terra_query::dispatch(cmd.as_bytes(), ContentFormat::Yaml, &registry, &self.store)
            .map_err(|e| String::from_utf8_lossy(&e.serialize(ContentFormat::Yaml)).into_owned())?;
        let val: serde_json::Value = serde_yaml::from_slice(&bytes).map_err(|e| e.to_string())?;
        let id_str = val["id"].as_str().ok_or("branch has no id")?;
        let branch_id: Uuid = id_str.parse().map_err(|e: uuid::Error| e.to_string())?;

        // Build ancestry from the branch response
        let mut ancestry = Vec::new();
        if let Some(arr) = val["ancestry"].as_array() {
            for item in arr {
                let bid: Uuid = item[0].as_str().unwrap_or_default().parse().unwrap_or(MAIN_BRANCH);
                let tid: Uuid = item[1].as_str().unwrap_or_default().parse().unwrap_or(Uuid::max());
                ancestry.push((bid, tid));
            }
        }
        ancestry.push((branch_id, Uuid::max()));

        Ok((branch_id, ancestry))
    }

    fn log_query(&self, label: &str, data: &str) {
        if let Some(ref path) = self.log_path {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(f, "\n--- {label} ---\n{data}");
            }
        }
    }
}
