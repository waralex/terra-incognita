use std::collections::HashMap;

use uuid::Uuid;

use crate::assertion::{
    AssertionInput, AssertionStore, EntityError, LogEntry, Transaction,
    WriterError,
};
use crate::schema::SchemaRegistry;

use super::AssertEntityInput;

/// Errors specific to the assert-entity business logic.
#[derive(Debug, thiserror::Error)]
pub enum AssertEntityError {
    /// Entity must exist for assertion but was not found.
    #[error("entity not found: {0}")]
    EntityNotFound(String),

    /// Entity already exists (during creation).
    #[error("entity already exists: {0}")]
    EntityAlreadyExists(String),

    /// Two facts in the same transaction assert the same property on the same entity type.
    #[error(
        "conflicting facts: property \"{property}\" on entity type \"{entity_type}\" \
         is asserted more than once in the same transaction — \
         if values are uncertain, use hypotheses instead of facts"
    )]
    ConflictingFacts {
        entity_type: String,
        property: String,
    },

    /// Referenced entity type not found in schema.
    #[error("entity type not found: {0}")]
    EntityTypeNotFound(String),

    /// Referenced property not found or not attached to entity type.
    #[error("property \"{property}\" not found on entity type \"{entity_type}\"")]
    PropertyNotFound {
        entity_type: String,
        property: String,
    },

    /// Entity storage error.
    #[error(transparent)]
    Entity(#[from] EntityError),

    /// Assertion writer error.
    #[error(transparent)]
    Writer(#[from] WriterError),

    /// Schema error.
    #[error(transparent)]
    Schema(#[from] crate::schema::SchemaError),
}

/// Result of assert-entity operation.
#[derive(Debug)]
pub struct AssertEntityResult {
    pub transaction: Transaction,
    pub facts: Vec<LogEntry>,
    pub hypotheses: Vec<LogEntry>,
}

/// Creates a new entity and optionally asserts facts/hypotheses.
pub fn create_entity(
    input: AssertEntityInput,
    registry: &SchemaRegistry,
    store: &AssertionStore,
) -> Result<AssertEntityResult, AssertEntityError> {
    // Validate BEFORE creating entity
    validate_no_conflicting_facts(&input.facts)?;
    let resolved_facts = resolve_items(&input.facts, Uuid::nil(), registry)?;
    let resolved_hypotheses = resolve_items(&input.hypotheses, Uuid::nil(), registry)?;

    // Check entity does NOT exist
    let entities = store.entities();
    if entities.get_by_slug(&input.entity)?.is_some() {
        return Err(AssertEntityError::EntityAlreadyExists(input.entity));
    }

    // Create entity
    let entity_record = entities.create(&input.entity, input.description.as_deref())?;

    // Write with real entity_id
    write_assertions(entity_record.id, input, resolved_facts, resolved_hypotheses, registry, store)
}

/// Asserts facts/hypotheses about an existing entity.
pub fn assert_entity(
    input: AssertEntityInput,
    registry: &SchemaRegistry,
    store: &AssertionStore,
) -> Result<AssertEntityResult, AssertEntityError> {
    // Validate first
    validate_no_conflicting_facts(&input.facts)?;
    let resolved_facts = resolve_items(&input.facts, Uuid::nil(), registry)?;
    let resolved_hypotheses = resolve_items(&input.hypotheses, Uuid::nil(), registry)?;

    // Check entity exists
    let entities = store.entities();
    let entity_record = entities
        .get_by_slug(&input.entity)?
        .ok_or_else(|| AssertEntityError::EntityNotFound(input.entity.clone()))?;

    write_assertions(entity_record.id, input, resolved_facts, resolved_hypotheses, registry, store)
}

fn write_assertions(
    entity_id: Uuid,
    input: AssertEntityInput,
    mut resolved_facts: Vec<AssertionInput>,
    mut resolved_hypotheses: Vec<AssertionInput>,
    registry: &SchemaRegistry,
    store: &AssertionStore,
) -> Result<AssertEntityResult, AssertEntityError> {
    // Fix entity_id (was Uuid::nil() during pre-validation resolve)
    for item in &mut resolved_facts {
        item.entity_id = entity_id;
    }
    for item in &mut resolved_hypotheses {
        item.entity_id = entity_id;
    }

    let fact_writer = store.fact_writer();
    let hyp_writer = store.hypothesis_writer();

    let (tx, facts) = if resolved_facts.is_empty() {
        let (tx, _) = fact_writer.write_tx(entity_id, input.reasoning.clone(), &[], registry)?;
        (tx, vec![])
    } else {
        fact_writer.write_tx(entity_id, input.reasoning.clone(), &resolved_facts, registry)?
    };

    let hypotheses = if resolved_hypotheses.is_empty() {
        vec![]
    } else {
        let (_, hyps) =
            hyp_writer.write_tx(entity_id, input.reasoning, &resolved_hypotheses, registry)?;
        hyps
    };

    Ok(AssertEntityResult {
        transaction: tx,
        facts,
        hypotheses,
    })
}

fn validate_no_conflicting_facts(
    facts: &[super::AssertionItem],
) -> Result<(), AssertEntityError> {
    for (idx, item) in facts.iter().enumerate() {
        for property_slug in item.properties.keys() {
            for prev_item in &facts[..idx] {
                if prev_item.entity_type == item.entity_type
                    && prev_item.properties.contains_key(property_slug)
                {
                    return Err(AssertEntityError::ConflictingFacts {
                        entity_type: item.entity_type.clone(),
                        property: property_slug.clone(),
                    });
                }
            }
        }
    }

    Ok(())
}

/// Resolves slug-based AssertionItems into UUID-based AssertionInputs.
fn resolve_items(
    items: &[super::AssertionItem],
    entity_id: Uuid,
    registry: &SchemaRegistry,
) -> Result<Vec<AssertionInput>, AssertEntityError> {
    let mut result = Vec::with_capacity(items.len());

    for item in items {
        let entity_type = registry.get_entity_type(&item.entity_type).map_err(|e| {
            match e {
                crate::schema::SchemaError::EntityTypeNotFound(_) => {
                    AssertEntityError::EntityTypeNotFound(item.entity_type.clone())
                }
                other => AssertEntityError::Schema(other),
            }
        })?;

        let attached_props = registry.list_properties_by_type_id(&entity_type.id)?;
        let prop_map: HashMap<&str, (Uuid, crate::schema::ValueType)> = attached_props
            .iter()
            .map(|p| (p.slug.as_str(), (p.id, p.value_type)))
            .collect();

        let mut properties = HashMap::with_capacity(item.properties.len());
        for (slug, value) in &item.properties {
            let (prop_id, _vt) = prop_map.get(slug.as_str()).ok_or_else(|| {
                AssertEntityError::PropertyNotFound {
                    entity_type: item.entity_type.clone(),
                    property: slug.clone(),
                }
            })?;
            properties.insert(*prop_id, value.clone());
        }

        result.push(AssertionInput {
            entity_id,
            entity_type_id: entity_type.id,
            properties,
            reasoning: item.reasoning.clone(),
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::{PropertyValue, RangeValue, SetValue};
    use crate::schema::ValueType;
    use serde_json::json;

    fn setup() -> (SchemaRegistry, AssertionStore, tempfile::TempDir) {
        let registry = SchemaRegistry::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (registry, store, dir)
    }

    fn setup_schema(reg: &SchemaRegistry) {
        reg.create_entity_type("track", None).unwrap();
        reg.create_property("bpm", ValueType::Range, None).unwrap();
        reg.create_property("certification", ValueType::Set, None)
            .unwrap();
        reg.attach_property("track", "bpm").unwrap();
        reg.attach_property("track", "certification").unwrap();
    }

    #[test]
    fn create_entity_with_facts() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = create_entity(
            AssertEntityInput {
                entity: "song-1".into(),
                description: Some("A great song".into()),
                reasoning: json!("initial analysis"),
                facts: vec![super::super::AssertionItem {
                    entity_type: "track".into(),
                    properties: HashMap::from([(
                        "bpm".into(),
                        PropertyValue::Range(RangeValue::Eq(json!(128))),
                    )]),
                    reasoning: json!("detected from waveform"),
                }],
                hypotheses: vec![],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.facts.len(), 1);
        assert!(result.hypotheses.is_empty());

        // Entity exists
        let entity = store.entities().get_by_slug("song-1").unwrap().unwrap();
        assert_eq!(entity.description.as_deref(), Some("A great song"));

        // Fact in log
        let log = store.facts().list().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].tx_id, Some(result.transaction.id));
    }

    #[test]
    fn create_entity_with_hypotheses() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = create_entity(
            AssertEntityInput {
                entity: "song-2".into(),
                description: None,
                reasoning: json!("exploring possibilities"),
                facts: vec![],
                hypotheses: vec![
                    super::super::AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(120))),
                        )]),
                        reasoning: json!("estimate A"),
                    },
                    super::super::AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(130))),
                        )]),
                        reasoning: json!("estimate B"),
                    },
                ],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert!(result.facts.is_empty());
        assert_eq!(result.hypotheses.len(), 2);

        let hyp_log = store.hypotheses().list().unwrap();
        assert_eq!(hyp_log.len(), 2);
    }

    #[test]
    fn assert_entity_existing() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        // Create entity first
        store.entities().create("song-3", None).unwrap();

        let result = assert_entity(
            AssertEntityInput {
                entity: "song-3".into(),
                description: None,
                reasoning: json!("follow-up analysis"),
                facts: vec![super::super::AssertionItem {
                    entity_type: "track".into(),
                    properties: HashMap::from([(
                        "certification".into(),
                        PropertyValue::Set(SetValue {
                            contains: vec![json!("gold")],
                            not_contains: vec![],
                        }),
                    )]),
                    reasoning: json!("confirmed by RIAA"),
                }],
                hypotheses: vec![],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.facts.len(), 1);
    }

    #[test]
    fn assert_entity_not_found() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = assert_entity(
            AssertEntityInput {
                entity: "nonexistent".into(),
                description: None,
                reasoning: json!(null),
                facts: vec![],
                hypotheses: vec![],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::EntityNotFound(_)));
    }

    #[test]
    fn create_entity_duplicate_fails() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        store.entities().create("dupe", None).unwrap();

        let err = create_entity(
            AssertEntityInput {
                entity: "dupe".into(),
                description: None,
                reasoning: json!(null),
                facts: vec![],
                hypotheses: vec![],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::EntityAlreadyExists(_)));
    }

    #[test]
    fn conflicting_facts_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = create_entity(
            AssertEntityInput {
                entity: "conflict".into(),
                description: None,
                reasoning: json!(null),
                facts: vec![
                    super::super::AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(120))),
                        )]),
                        reasoning: json!("analysis A"),
                    },
                    super::super::AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(130))),
                        )]),
                        reasoning: json!("analysis B"),
                    },
                ],
                hypotheses: vec![],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::ConflictingFacts { .. }));

        // Entity should NOT have been created
        assert!(store.entities().get_by_slug("conflict").unwrap().is_none());
    }

    #[test]
    fn unknown_entity_type_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = create_entity(
            AssertEntityInput {
                entity: "bad-type".into(),
                description: None,
                reasoning: json!(null),
                facts: vec![super::super::AssertionItem {
                    entity_type: "nonexistent-type".into(),
                    properties: HashMap::new(),
                    reasoning: json!(null),
                }],
                hypotheses: vec![],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::EntityTypeNotFound(_)));
    }

    #[test]
    fn unknown_property_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = create_entity(
            AssertEntityInput {
                entity: "bad-prop".into(),
                description: None,
                reasoning: json!(null),
                facts: vec![super::super::AssertionItem {
                    entity_type: "track".into(),
                    properties: HashMap::from([(
                        "nonexistent-prop".into(),
                        PropertyValue::Range(RangeValue::Eq(json!(0))),
                    )]),
                    reasoning: json!(null),
                }],
                hypotheses: vec![],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::PropertyNotFound { .. }));
    }

    #[test]
    fn mixed_facts_and_hypotheses() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = create_entity(
            AssertEntityInput {
                entity: "mixed".into(),
                description: Some("Mixed assertions".into()),
                reasoning: json!("comprehensive analysis"),
                facts: vec![super::super::AssertionItem {
                    entity_type: "track".into(),
                    properties: HashMap::from([(
                        "certification".into(),
                        PropertyValue::Set(SetValue {
                            contains: vec![json!("gold")],
                            not_contains: vec![],
                        }),
                    )]),
                    reasoning: json!("confirmed"),
                }],
                hypotheses: vec![
                    super::super::AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(120))),
                        )]),
                        reasoning: json!("estimate A"),
                    },
                    super::super::AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(128))),
                        )]),
                        reasoning: json!("estimate B"),
                    },
                ],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.facts.len(), 1);
        assert_eq!(result.hypotheses.len(), 2);

        // Facts and hypotheses in separate logs
        assert_eq!(store.facts().list().unwrap().len(), 1);
        assert_eq!(store.hypotheses().list().unwrap().len(), 2);
    }

    #[test]
    fn same_property_in_facts_and_hypotheses_allowed() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        // bpm as fact AND bpm as hypothesis — this is allowed
        // (fact = convergence, hypothesis = alternative under consideration)
        let result = create_entity(
            AssertEntityInput {
                entity: "overlap".into(),
                description: None,
                reasoning: json!("mixed certainty"),
                facts: vec![super::super::AssertionItem {
                    entity_type: "track".into(),
                    properties: HashMap::from([(
                        "bpm".into(),
                        PropertyValue::Range(RangeValue::Eq(json!(120))),
                    )]),
                    reasoning: json!("most likely"),
                }],
                hypotheses: vec![super::super::AssertionItem {
                    entity_type: "track".into(),
                    properties: HashMap::from([(
                        "bpm".into(),
                        PropertyValue::Range(RangeValue::Eq(json!(125))),
                    )]),
                    reasoning: json!("but maybe this"),
                }],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.facts.len(), 1);
        assert_eq!(result.hypotheses.len(), 1);
    }
}
