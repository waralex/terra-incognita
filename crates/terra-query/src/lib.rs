mod dispatch;
pub mod error;
pub mod format;
mod query;
mod response;

pub use dispatch::dispatch;
pub use error::QueryError;
pub use format::ContentFormat;

#[cfg(test)]
mod tests {
    use super::*;
    use terra_core::assertion::{AssertionStore, MAIN_BRANCH};
    use terra_core::schema::BranchSchemaRegistry;
    use uuid::Uuid;

    fn setup() -> (BranchSchemaRegistry, AssertionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        let registry = store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
        (registry, store, dir)
    }

    fn dispatch_yaml(
        yaml: &str,
        registry: &BranchSchemaRegistry,
        store: &AssertionStore,
    ) -> Result<serde_json::Value, QueryError> {
        let bytes = dispatch(yaml.as_bytes(), ContentFormat::Yaml, registry, store)?;
        Ok(serde_yaml::from_slice(&bytes).unwrap())
    }

    #[test]
    fn entity_list_returns_created_entities() {
        let (reg, store, _dir) = setup();

        // Setup schema and create entity via unified transaction
        dispatch_yaml(
            "command: transaction\nentity_types:\n  - slug: track\n    properties:\n      - slug: bpm\n        value_type: range\nintroduce:\n  - entity: song-1\n    entity_type: track\n",
            &reg,
            &store,
        )
        .unwrap();

        // List entities
        let result = dispatch_yaml(
            "command: entity.list\n",
            &reg,
            &store,
        )
        .unwrap();

        let arr = result.as_array().expect("entity.list should return array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["slug"], "song-1");
    }

    #[test]
    fn json_format_roundtrip() {
        let (reg, store, _dir) = setup();

        let input = br#"{"command": "transaction", "entity_types": [{"slug": "track"}]}"#;
        let bytes = dispatch(input, ContentFormat::Json, &reg, &store).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["entity_types"][0]["slug"], "track");
    }
}
