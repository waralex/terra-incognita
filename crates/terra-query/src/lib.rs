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
    use terra_core::assertion::AssertionStore;
    use terra_core::schema::SchemaRegistry;

    fn setup() -> (SchemaRegistry, AssertionStore, tempfile::TempDir) {
        let registry = SchemaRegistry::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (registry, store, dir)
    }

    fn dispatch_yaml(
        yaml: &str,
        registry: &mut SchemaRegistry,
        store: &AssertionStore,
    ) -> Result<serde_json::Value, QueryError> {
        let bytes = dispatch(yaml.as_bytes(), ContentFormat::Yaml, registry, store)?;
        Ok(serde_yaml::from_slice(&bytes).unwrap())
    }

    #[test]
    fn entity_list_returns_created_entities() {
        let (mut reg, store, _dir) = setup();

        // Setup schema
        dispatch_yaml(
            "command: entity-type.create\nslug: track\n",
            &mut reg,
            &store,
        )
        .unwrap();
        dispatch_yaml(
            "command: property.create\nslug: bpm\nvalue_type: range\n",
            &mut reg,
            &store,
        )
        .unwrap();
        dispatch_yaml(
            "command: property.attach\nentity_type: track\nslug: bpm\n",
            &mut reg,
            &store,
        )
        .unwrap();

        // Create entity
        dispatch_yaml(
            "command: entity.create\nentity: song-1\n",
            &mut reg,
            &store,
        )
        .unwrap();

        // List entities
        let result = dispatch_yaml(
            "command: entity.list\n",
            &mut reg,
            &store,
        )
        .unwrap();

        let arr = result.as_array().expect("entity.list should return array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["slug"], "song-1");
    }

    #[test]
    fn json_format_roundtrip() {
        let (mut reg, store, _dir) = setup();

        let input = br#"{"command": "entity-type.create", "slug": "track"}"#;
        let bytes = dispatch(input, ContentFormat::Json, &mut reg, &store).unwrap();
        let val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(val["slug"], "track");
    }
}
