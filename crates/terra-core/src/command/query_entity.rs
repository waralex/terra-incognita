use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::assertion::AssertionStore;
use crate::schema::{EntityProperty, BranchSchemaRegistry, ValueType};

/// State of a single property in an entity projection.
#[derive(Debug, Clone, Serialize)]
pub struct PropertyState {
    pub slug: String,
    pub value_type: ValueType,
    /// Latest fact value, or null if no fact exists.
    pub value: serde_json::Value,
    /// Whether any fact has been recorded for this property.
    pub known: bool,
    /// Number of hypotheses recorded after the latest fact.
    pub pending: usize,
}

/// Full entity projection onto an entity type.
#[derive(Debug, Serialize)]
pub struct EntityProjection {
    pub entity_id: Uuid,
    pub entity_slug: String,
    pub entity_type: String,
    pub properties: Vec<PropertyState>,
}

/// Builds an entity projection: for each property of the given entity type,
/// finds the latest fact value and counts pending hypotheses.
pub fn project_entity(
    entity_slug: &str,
    entity_type_slug: &str,
    registry: &BranchSchemaRegistry,
    store: &AssertionStore,
) -> Result<EntityProjection, ProjectionError> {
    let entity = store
        .entities()
        .get_by_slug(entity_slug)?
        .ok_or_else(|| ProjectionError::EntityNotFound(entity_slug.to_string()))?;

    let entity_type = registry.get_entity_type(entity_type_slug)?;
    let attached_props = registry.list_properties_by_type_id(&entity_type.id)?;

    let properties = attached_props
        .iter()
        .map(|prop| resolve_property_state(entity.id, prop, store))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(EntityProjection {
        entity_id: entity.id,
        entity_slug: entity.slug,
        entity_type: entity_type_slug.to_string(),
        properties,
    })
}

fn resolve_property_state(
    entity_id: Uuid,
    prop: &EntityProperty,
    store: &AssertionStore,
) -> Result<PropertyState, ProjectionError> {
    let fact_col = match prop.value_type {
        ValueType::Set => store.fact_col_set(),
        ValueType::Struct => store.fact_col_struct(),
        ValueType::Range => store.fact_col_range(),
    };

    let hyp_col = match prop.value_type {
        ValueType::Set => store.hypothesis_col_set(),
        ValueType::Struct => store.hypothesis_col_struct(),
        ValueType::Range => store.hypothesis_col_range(),
    };

    let latest_fact = fact_col.latest_for_entity(prop.id, entity_id)?;

    let (value, known, pending) = match latest_fact {
        Some(cell) => {
            let pending = hyp_col.count_after(prop.id, entity_id, cell.log_entry_id)?;
            (cell.value, true, pending)
        }
        None => {
            // No fact — count all hypotheses
            let pending = hyp_col.count_after(prop.id, entity_id, Uuid::nil())?;
            (json!(null), false, pending)
        }
    };

    Ok(PropertyState {
        slug: prop.slug.clone(),
        value_type: prop.value_type,
        value,
        known,
        pending,
    })
}

/// Errors from entity projection.
#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("entity not found: {0}")]
    EntityNotFound(String),

    #[error(transparent)]
    Entity(#[from] crate::assertion::EntityError),

    #[error(transparent)]
    Schema(#[from] crate::schema::SchemaError),

    #[error(transparent)]
    Storage(#[from] crate::assertion::LogError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::{PropertyValue, RangeValue, SetValue, MAIN_BRANCH};
    use crate::command::assert_entity;
    use crate::command::{AssertionItem, HideUnhideInput, IntroduceItem, AssertItem, TransactionInput};
    use std::collections::HashMap;

    fn setup() -> (BranchSchemaRegistry, AssertionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        let registry = store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
        (registry, store, dir)
    }

    fn setup_schema(reg: &BranchSchemaRegistry) {
        reg.create_entity_type("track", None).unwrap();
        reg.create_property("bpm", ValueType::Range, None).unwrap();
        reg.create_property("certification", ValueType::Set, None)
            .unwrap();
        reg.attach_property("track", "bpm").unwrap();
        reg.attach_property("track", "certification").unwrap();
    }

    fn tx_input(
        reasoning: serde_json::Value,
        introduce: Vec<IntroduceItem>,
        asserts: Vec<AssertItem>,
    ) -> TransactionInput {
        TransactionInput {
            reasoning,
            entity_types: vec![],
            properties: vec![],
            attach: vec![],
            hide: HideUnhideInput::default(),
            unhide: HideUnhideInput::default(),
            introduce,
            asserts,
        }
    }

    #[test]
    fn project_entity_with_facts() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        assert_entity::execute_transaction(
            tx_input(
                json!("test"),
                vec![IntroduceItem {
                    entity: "song-1".into(),
                    description: None,
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([
                            (
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(json!(128))),
                            ),
                            (
                                "certification".into(),
                                PropertyValue::Set(SetValue {
                                    contains: vec![json!("gold")],
                                    not_contains: vec![],
                                }),
                            ),
                        ]),
                        reasoning: json!("analysis"),
                    }],
                    hypotheses: vec![],
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        let proj = project_entity("song-1", "track", &reg, &store).unwrap();
        assert_eq!(proj.entity_slug, "song-1");
        assert_eq!(proj.entity_type, "track");
        assert_eq!(proj.properties.len(), 2);

        for prop in &proj.properties {
            assert!(prop.known);
            assert_eq!(prop.pending, 0);
        }
    }

    #[test]
    fn project_entity_unknown_properties() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        assert_entity::execute_transaction(
            tx_input(
                json!("empty"),
                vec![IntroduceItem {
                    entity: "song-2".into(),
                    description: None,
                    facts: vec![],
                    hypotheses: vec![],
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        let proj = project_entity("song-2", "track", &reg, &store).unwrap();
        for prop in &proj.properties {
            assert!(!prop.known);
            assert_eq!(prop.value, json!(null));
            assert_eq!(prop.pending, 0);
        }
    }

    #[test]
    fn project_entity_with_pending_hypotheses() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        // Create with a fact
        assert_entity::execute_transaction(
            tx_input(
                json!("initial"),
                vec![IntroduceItem {
                    entity: "song-3".into(),
                    description: None,
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(120))),
                        )]),
                        reasoning: json!("detected"),
                    }],
                    hypotheses: vec![],
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        // Add hypotheses after the fact
        assert_entity::execute_transaction(
            tx_input(
                json!("re-analysis"),
                vec![],
                vec![AssertItem {
                    entity: "song-3".into(),
                    facts: vec![],
                    hypotheses: vec![
                        AssertionItem {
                            entity_type: "track".into(),
                            properties: HashMap::from([(
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(json!(122))),
                            )]),
                            reasoning: json!("maybe higher"),
                        },
                        AssertionItem {
                            entity_type: "track".into(),
                            properties: HashMap::from([(
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(json!(118))),
                            )]),
                            reasoning: json!("maybe lower"),
                        },
                    ],
                }],
            ),
            &reg,
            &store,
        )
        .unwrap();

        let proj = project_entity("song-3", "track", &reg, &store).unwrap();
        let bpm = proj.properties.iter().find(|p| p.slug == "bpm").unwrap();
        assert!(bpm.known);
        assert_eq!(bpm.value, json!({"eq": 120}));
        assert_eq!(bpm.pending, 2);

        let cert = proj
            .properties
            .iter()
            .find(|p| p.slug == "certification")
            .unwrap();
        assert!(!cert.known);
        assert_eq!(cert.pending, 0);
    }

    #[test]
    fn project_entity_hypotheses_without_fact() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        assert_entity::execute_transaction(
            tx_input(
                json!("guessing"),
                vec![IntroduceItem {
                    entity: "song-4".into(),
                    description: None,
                    facts: vec![],
                    hypotheses: vec![
                        AssertionItem {
                            entity_type: "track".into(),
                            properties: HashMap::from([(
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(json!(100))),
                            )]),
                            reasoning: json!("guess 1"),
                        },
                        AssertionItem {
                            entity_type: "track".into(),
                            properties: HashMap::from([(
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(json!(140))),
                            )]),
                            reasoning: json!("guess 2"),
                        },
                    ],
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        let proj = project_entity("song-4", "track", &reg, &store).unwrap();
        let bpm = proj.properties.iter().find(|p| p.slug == "bpm").unwrap();
        assert!(!bpm.known);
        assert_eq!(bpm.pending, 2);
    }

    #[test]
    fn project_entity_not_found() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = project_entity("nonexistent", "track", &reg, &store).unwrap_err();
        assert!(matches!(err, ProjectionError::EntityNotFound(_)));
    }

    #[test]
    fn project_entity_type_not_found() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        store.entities().create("song-5", None).unwrap();

        let err = project_entity("song-5", "nonexistent", &reg, &store).unwrap_err();
        assert!(matches!(err, ProjectionError::Schema(_)));
    }
}
