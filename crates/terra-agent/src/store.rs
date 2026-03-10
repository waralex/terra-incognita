use std::path::Path;
use terra_core::assertion::{AssertionStore, MAIN_BRANCH};
use terra_query::ContentFormat;
use uuid::Uuid;

/// Thin wrapper around AssertionStore for dispatching YAML queries.
pub struct StoreHandle {
    store: AssertionStore,
}

impl StoreHandle {
    /// Opens an AssertionStore at the given path.
    pub fn open(path: &Path) -> Self {
        let store = AssertionStore::open(path).expect("failed to open assertion store");
        Self { store }
    }

    /// Dispatches a YAML command string and returns the YAML response.
    pub fn dispatch(&self, input: &str, branch: &str) -> Result<String, String> {
        let (branch_id, ancestry) = self.resolve_branch(branch)?;
        let registry = self.store.schema_registry(branch_id, ancestry);
        let bytes = terra_query::dispatch(input.as_bytes(), ContentFormat::Yaml, &registry, &self.store)
            .map_err(|e| String::from_utf8_lossy(&e.serialize(ContentFormat::Yaml)).into_owned())?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Fetches branch state as YAML for the side panel.
    pub fn fetch_state(&self, branch: &str) -> Result<String, String> {
        let cmd = format!("command: branch.state\nbranch: {branch}");
        self.dispatch(&cmd, branch)
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
}
