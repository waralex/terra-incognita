use std::collections::HashMap;

use uuid::Uuid;

use crate::assertion::{
    AssertionInput, AssertionStore, EntityError, LogEntry, Transaction,
    WriterError,
};
use crate::schema::SchemaRegistry;

use super::{AssertEntityInput, TransactionEntityResult, TransactionInput};

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

/// Result of a multi-entity transaction.
#[derive(Debug)]
pub struct TransactionExecResult {
    pub transaction: Transaction,
    pub introduced: Vec<TransactionEntityResult>,
    pub asserted: Vec<TransactionEntityResult>,
}

/// Executes a multi-entity transaction: introduces new entities, then asserts on existing ones.
///
/// All operations share one transaction record and one WriteBatch — if anything
/// fails validation, nothing is written.
pub fn execute_transaction(
    input: TransactionInput,
    registry: &SchemaRegistry,
    store: &AssertionStore,
) -> Result<TransactionExecResult, AssertEntityError> {
    let entities = store.entities();

    // Phase 1: Validate everything before any mutation

    // Validate introduces
    let mut intro_resolved: Vec<(String, Option<String>, Vec<AssertionInput>, Vec<AssertionInput>)> =
        Vec::with_capacity(input.introduce.len());
    for item in &input.introduce {
        validate_no_conflicting_facts(&item.facts)?;
        let facts = resolve_items(&item.facts, Uuid::nil(), registry)?;
        let hyps = resolve_items(&item.hypotheses, Uuid::nil(), registry)?;
        if entities.get_by_slug(&item.entity)?.is_some() {
            return Err(AssertEntityError::EntityAlreadyExists(item.entity.clone()));
        }
        intro_resolved.push((
            item.entity.clone(),
            item.description.clone(),
            facts,
            hyps,
        ));
    }

    // Check for duplicate slugs within introduces
    for (i, item) in input.introduce.iter().enumerate() {
        for prev in &input.introduce[..i] {
            if prev.entity == item.entity {
                return Err(AssertEntityError::EntityAlreadyExists(item.entity.clone()));
            }
        }
    }

    // Validate asserts
    let mut assert_resolved: Vec<(Uuid, String, Vec<AssertionInput>, Vec<AssertionInput>)> =
        Vec::with_capacity(input.asserts.len());
    for item in &input.asserts {
        validate_no_conflicting_facts(&item.facts)?;
        let facts = resolve_items(&item.facts, Uuid::nil(), registry)?;
        let hyps = resolve_items(&item.hypotheses, Uuid::nil(), registry)?;

        // Entity must exist OR be in the introduce list
        if let Some(record) = entities.get_by_slug(&item.entity)? {
            assert_resolved.push((record.id, item.entity.clone(), facts, hyps));
        } else if input.introduce.iter().any(|i| i.entity == item.entity) {
            // Will be resolved after entity creation — use nil placeholder
            assert_resolved.push((Uuid::nil(), item.entity.clone(), facts, hyps));
        } else {
            return Err(AssertEntityError::EntityNotFound(item.entity.clone()));
        }
    }

    // Phase 2: Create entities from introduce list
    let mut intro_entities: Vec<(Uuid, String)> = Vec::with_capacity(intro_resolved.len());
    for (slug, desc, _, _) in &intro_resolved {
        let record = entities.create(slug, desc.as_deref())?;
        intro_entities.push((record.id, slug.clone()));
    }

    // Fix entity_ids for introduce resolved items
    for ((entity_id, _), (_, _, facts, hyps)) in intro_entities.iter().zip(intro_resolved.iter_mut())
    {
        for item in facts.iter_mut().chain(hyps.iter_mut()) {
            item.entity_id = *entity_id;
        }
    }

    // Fix entity_ids for asserts that reference introduced entities
    for (entity_id, slug, facts, hyps) in assert_resolved.iter_mut() {
        if *entity_id == Uuid::nil() {
            let real_id = intro_entities
                .iter()
                .find(|(_, s)| s == slug)
                .map(|(id, _)| *id)
                .expect("introduced entity must exist at this point");
            *entity_id = real_id;
            for item in facts.iter_mut().chain(hyps.iter_mut()) {
                item.entity_id = real_id;
            }
        }
    }

    // Phase 3: Build one WriteBatch for all assertions
    let tx = Transaction {
        id: Uuid::now_v7(),
        entity_id: None,
        reasoning: input.reasoning,
        timestamp: chrono::Utc::now(),
    };

    let mut batch = rocksdb::WriteBatch::default();
    store
        .transactions()
        .put_to_batch(&mut batch, &tx)
        .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;

    let fact_writer = store.fact_writer();
    let hyp_writer = store.hypothesis_writer();

    // Write introduces
    let mut introduced = Vec::with_capacity(intro_resolved.len());
    for ((entity_id, slug), (_, _, facts, hyps)) in
        intro_entities.iter().zip(intro_resolved.iter())
    {
        let fact_entries = if facts.is_empty() {
            vec![]
        } else {
            fact_writer.write_to_batch(&mut batch, tx.id, facts, registry)?
        };
        let hyp_entries = if hyps.is_empty() {
            vec![]
        } else {
            hyp_writer.write_to_batch(&mut batch, tx.id, hyps, registry)?
        };
        introduced.push(TransactionEntityResult {
            entity_id: *entity_id,
            entity_slug: slug.clone(),
            facts: fact_entries,
            hypotheses: hyp_entries,
        });
    }

    // Write asserts
    let mut asserted = Vec::with_capacity(assert_resolved.len());
    for (entity_id, slug, facts, hyps) in &assert_resolved {
        let fact_entries = if facts.is_empty() {
            vec![]
        } else {
            fact_writer.write_to_batch(&mut batch, tx.id, facts, registry)?
        };
        let hyp_entries = if hyps.is_empty() {
            vec![]
        } else {
            hyp_writer.write_to_batch(&mut batch, tx.id, hyps, registry)?
        };
        asserted.push(TransactionEntityResult {
            entity_id: *entity_id,
            entity_slug: slug.clone(),
            facts: fact_entries,
            hypotheses: hyp_entries,
        });
    }

    // Commit everything atomically
    store
        .write_batch(batch)
        .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;

    Ok(TransactionExecResult {
        transaction: tx,
        introduced,
        asserted,
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

    fn make_item(et: &str, prop: &str, value: PropertyValue) -> super::super::AssertionItem {
        super::super::AssertionItem {
            entity_type: et.into(),
            properties: HashMap::from([(prop.into(), value)]),
            reasoning: json!(null),
        }
    }

    // -- Transaction tests --

    #[test]
    fn transaction_introduce_multiple_entities() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = execute_transaction(
            TransactionInput {
                reasoning: json!("catalog import"),
                introduce: vec![
                    super::super::IntroduceItem {
                        entity: "song-a".into(),
                        description: Some("First song".into()),
                        facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(120))))],
                        hypotheses: vec![],
                    },
                    super::super::IntroduceItem {
                        entity: "song-b".into(),
                        description: None,
                        facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(140))))],
                        hypotheses: vec![],
                    },
                ],
                asserts: vec![],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert!(result.transaction.entity_id.is_none());
        assert_eq!(result.introduced.len(), 2);
        assert_eq!(result.asserted.len(), 0);
        assert_eq!(result.introduced[0].entity_slug, "song-a");
        assert_eq!(result.introduced[1].entity_slug, "song-b");
        assert_eq!(result.introduced[0].facts.len(), 1);
        assert_eq!(result.introduced[1].facts.len(), 1);

        // All log entries share the same tx_id
        let tx_id = result.transaction.id;
        assert!(result.introduced[0].facts[0].tx_id == Some(tx_id));
        assert!(result.introduced[1].facts[0].tx_id == Some(tx_id));

        // Entities exist
        assert!(store.entities().get_by_slug("song-a").unwrap().is_some());
        assert!(store.entities().get_by_slug("song-b").unwrap().is_some());
    }

    #[test]
    fn transaction_assert_on_existing_entity() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        store.entities().create("existing-song", None).unwrap();

        let result = execute_transaction(
            TransactionInput {
                reasoning: json!("follow-up"),
                introduce: vec![],
                asserts: vec![super::super::AssertItem {
                    entity: "existing-song".into(),
                    facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(100))))],
                    hypotheses: vec![],
                }],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.introduced.len(), 0);
        assert_eq!(result.asserted.len(), 1);
        assert_eq!(result.asserted[0].entity_slug, "existing-song");
        assert_eq!(result.asserted[0].facts.len(), 1);
    }

    #[test]
    fn transaction_assert_references_introduced_entity() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = execute_transaction(
            TransactionInput {
                reasoning: json!("create and annotate"),
                introduce: vec![super::super::IntroduceItem {
                    entity: "new-song".into(),
                    description: Some("Brand new".into()),
                    facts: vec![],
                    hypotheses: vec![],
                }],
                asserts: vec![super::super::AssertItem {
                    entity: "new-song".into(),
                    facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(130))))],
                    hypotheses: vec![],
                }],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.introduced.len(), 1);
        assert_eq!(result.asserted.len(), 1);
        // Assert got the real entity_id from the introduce
        assert_eq!(result.introduced[0].entity_id, result.asserted[0].entity_id);
    }

    #[test]
    fn transaction_duplicate_introduce_slugs_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = execute_transaction(
            TransactionInput {
                reasoning: json!(null),
                introduce: vec![
                    super::super::IntroduceItem {
                        entity: "dupe".into(),
                        description: None,
                        facts: vec![],
                        hypotheses: vec![],
                    },
                    super::super::IntroduceItem {
                        entity: "dupe".into(),
                        description: None,
                        facts: vec![],
                        hypotheses: vec![],
                    },
                ],
                asserts: vec![],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::EntityAlreadyExists(_)));
    }

    #[test]
    fn transaction_assert_nonexistent_entity_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = execute_transaction(
            TransactionInput {
                reasoning: json!(null),
                introduce: vec![],
                asserts: vec![super::super::AssertItem {
                    entity: "ghost".into(),
                    facts: vec![],
                    hypotheses: vec![],
                }],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::EntityNotFound(_)));
    }

    #[test]
    fn transaction_conflicting_facts_in_introduce_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = execute_transaction(
            TransactionInput {
                reasoning: json!(null),
                introduce: vec![super::super::IntroduceItem {
                    entity: "conflict".into(),
                    description: None,
                    facts: vec![
                        make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(120)))),
                        make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(130)))),
                    ],
                    hypotheses: vec![],
                }],
                asserts: vec![],
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
    fn transaction_introduce_already_exists_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        store.entities().create("taken", None).unwrap();

        let err = execute_transaction(
            TransactionInput {
                reasoning: json!(null),
                introduce: vec![super::super::IntroduceItem {
                    entity: "taken".into(),
                    description: None,
                    facts: vec![],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::EntityAlreadyExists(_)));
    }

    #[test]
    fn transaction_mixed_introduce_and_assert_with_hypotheses() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = execute_transaction(
            TransactionInput {
                reasoning: json!("comprehensive import"),
                introduce: vec![super::super::IntroduceItem {
                    entity: "song-x".into(),
                    description: Some("A song".into()),
                    facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(128))))],
                    hypotheses: vec![make_item("track", "certification", PropertyValue::Set(SetValue {
                        contains: vec![json!("gold")],
                        not_contains: vec![],
                    }))],
                }],
                asserts: vec![super::super::AssertItem {
                    entity: "song-x".into(),
                    facts: vec![],
                    hypotheses: vec![make_item("track", "certification", PropertyValue::Set(SetValue {
                        contains: vec![json!("platinum")],
                        not_contains: vec![],
                    }))],
                }],
            },
            &reg,
            &store,
        )
        .unwrap();

        // Introduce: 1 fact, 1 hypothesis
        assert_eq!(result.introduced[0].facts.len(), 1);
        assert_eq!(result.introduced[0].hypotheses.len(), 1);
        // Assert: 0 facts, 1 hypothesis
        assert_eq!(result.asserted[0].facts.len(), 0);
        assert_eq!(result.asserted[0].hypotheses.len(), 1);

        // All share same tx_id
        let tx_id = result.transaction.id;
        for entry in &result.introduced[0].facts {
            assert_eq!(entry.tx_id, Some(tx_id));
        }
        for entry in &result.asserted[0].hypotheses {
            assert_eq!(entry.tx_id, Some(tx_id));
        }
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
