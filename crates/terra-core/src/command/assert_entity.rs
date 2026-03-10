use std::collections::HashMap;

use uuid::Uuid;

use crate::assertion::{
    AssertionInput, AssertionStore, EntityError, ItemKind, Transaction,
    WriterError,
};
use crate::schema::{AttachInput, BranchSchemaRegistry, EntityProperty, EntityType, EntityTypeInput, PropertyInput};

use super::{TransactionEntityResult, TransactionInput};

/// Errors specific to the transaction business logic.
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

/// Result of executing a unified transaction.
#[derive(Debug)]
pub struct TransactionExecResult {
    pub transaction: Transaction,
    pub entity_types: Vec<EntityType>,
    pub properties: Vec<EntityProperty>,
    pub attached_count: usize,
    pub introduced: Vec<TransactionEntityResult>,
    pub asserted: Vec<TransactionEntityResult>,
}

/// Executes a unified transaction: schema operations, visibility changes,
/// entity introduction, and assertions — all in one command.
///
/// Processing order:
/// 1. Create entity types (committed via registry)
/// 2. Create properties (committed via registry)
/// 3. Attach properties (committed via registry)
/// 4. Build one WriteBatch for: Transaction record + visibility + assertions
/// 5. Introduce new entities + write their assertions
/// 6. Write assertions on existing entities
pub fn execute_transaction(
    input: TransactionInput,
    registry: &BranchSchemaRegistry,
    store: &AssertionStore,
) -> Result<TransactionExecResult, AssertEntityError> {
    // Phase 0: Schema operations (committed independently via registry)

    let created_entity_types = if input.entity_types.is_empty() {
        vec![]
    } else {
        let prop_strs: Vec<Vec<&str>> = input
            .entity_types
            .iter()
            .map(|item| item.properties.iter().map(|s| s.as_str()).collect())
            .collect();
        let inputs: Vec<EntityTypeInput<'_>> = input
            .entity_types
            .iter()
            .zip(prop_strs.iter())
            .map(|(item, props)| EntityTypeInput {
                slug: &item.slug,
                description: item.description.as_deref(),
                properties: props,
            })
            .collect();
        registry.create_entity_types_batch(&inputs)?
    };

    let created_properties = if input.properties.is_empty() {
        vec![]
    } else {
        let et_strs: Vec<Vec<&str>> = input
            .properties
            .iter()
            .map(|item| item.entity_types.iter().map(|s| s.as_str()).collect())
            .collect();
        let inputs: Vec<PropertyInput<'_>> = input
            .properties
            .iter()
            .zip(et_strs.iter())
            .map(|(item, ets)| PropertyInput {
                slug: &item.slug,
                value_type: item.value_type,
                description: item.description.as_deref(),
                entity_types: ets,
            })
            .collect();
        registry.create_properties_batch(&inputs)?
    };

    let attached_count = if input.attach.is_empty() {
        0
    } else {
        let inputs: Vec<AttachInput<'_>> = input
            .attach
            .iter()
            .map(|item| AttachInput {
                entity_type: &item.entity_type,
                property: &item.property,
            })
            .collect();
        registry.attach_properties_batch(&inputs)?
    };

    // Phase 1: Validate assertions before any entity mutation

    let entities = store.entities();

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

    for (i, item) in input.introduce.iter().enumerate() {
        for prev in &input.introduce[..i] {
            if prev.entity == item.entity {
                return Err(AssertEntityError::EntityAlreadyExists(item.entity.clone()));
            }
        }
    }

    let mut assert_resolved: Vec<(Uuid, String, Vec<AssertionInput>, Vec<AssertionInput>)> =
        Vec::with_capacity(input.asserts.len());
    for item in &input.asserts {
        validate_no_conflicting_facts(&item.facts)?;
        let facts = resolve_items(&item.facts, Uuid::nil(), registry)?;
        let hyps = resolve_items(&item.hypotheses, Uuid::nil(), registry)?;

        if let Some(record) = entities.get_by_slug(&item.entity)? {
            assert_resolved.push((record.id, item.entity.clone(), facts, hyps));
        } else if input.introduce.iter().any(|i| i.entity == item.entity) {
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

    // Fix entity_ids for all assert resolved items.
    // resolve_items used Uuid::nil() as placeholder; now replace with the real entity_id.
    for (entity_id, slug, facts, hyps) in assert_resolved.iter_mut() {
        if *entity_id == Uuid::nil() {
            // References an introduced entity — look up its real id
            let real_id = intro_entities
                .iter()
                .find(|(_, s)| s == slug)
                .map(|(id, _)| *id)
                .expect("introduced entity must exist at this point");
            *entity_id = real_id;
        }
        for item in facts.iter_mut().chain(hyps.iter_mut()) {
            item.entity_id = *entity_id;
        }
    }

    // Phase 3: Build one WriteBatch for Transaction record + visibility + assertions
    let tx = Transaction {
        id: Uuid::now_v7(),
        branch_id: crate::assertion::MAIN_BRANCH,
        reasoning: input.reasoning,
        timestamp: chrono::Utc::now(),
    };

    let mut batch = rocksdb::WriteBatch::default();
    store
        .transactions()
        .put_to_batch(&mut batch, &tx)
        .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;

    // Visibility: hide
    let vis = store.visibility();
    resolve_and_write_visibility(
        &mut batch,
        &vis,
        tx.branch_id,
        tx.id,
        &input.hide,
        true,
        registry,
        &entities,
    )?;
    // Visibility: unhide
    resolve_and_write_visibility(
        &mut batch,
        &vis,
        tx.branch_id,
        tx.id,
        &input.unhide,
        false,
        registry,
        &entities,
    )?;

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
        entity_types: created_entity_types,
        properties: created_properties,
        attached_count,
        introduced,
        asserted,
    })
}

/// Resolves slug-based hide/unhide items to UUIDs and writes visibility records to the batch.
fn resolve_and_write_visibility(
    batch: &mut rocksdb::WriteBatch,
    vis: &crate::assertion::VisibilityStore,
    branch_id: Uuid,
    tx_id: Uuid,
    input: &super::HideUnhideInput,
    hide: bool,
    registry: &BranchSchemaRegistry,
    entities: &crate::assertion::EntityStore,
) -> Result<(), AssertEntityError> {
    // Resolve entity slugs
    if !input.entities.is_empty() {
        let mut ids = Vec::with_capacity(input.entities.len());
        for slug in &input.entities {
            let record = entities
                .get_by_slug(slug)?
                .ok_or_else(|| AssertEntityError::EntityNotFound(slug.clone()))?;
            ids.push(record.id);
        }
        if hide {
            vis.hide_to_batch(batch, branch_id, tx_id, ItemKind::Entity, &ids)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;
        } else {
            vis.unhide_to_batch(batch, branch_id, tx_id, ItemKind::Entity, &ids)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;
        }
    }

    // Resolve entity type slugs
    if !input.entity_types.is_empty() {
        let mut ids = Vec::with_capacity(input.entity_types.len());
        for slug in &input.entity_types {
            let et = registry.get_entity_type(slug)?;
            ids.push(et.id);
        }
        if hide {
            vis.hide_to_batch(batch, branch_id, tx_id, ItemKind::EntityType, &ids)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;
        } else {
            vis.unhide_to_batch(batch, branch_id, tx_id, ItemKind::EntityType, &ids)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;
        }
    }

    // Resolve property slugs
    if !input.properties.is_empty() {
        let mut ids = Vec::with_capacity(input.properties.len());
        for slug in &input.properties {
            let prop = registry.get_property_by_slug(slug)?;
            ids.push(prop.id);
        }
        if hide {
            vis.hide_to_batch(batch, branch_id, tx_id, ItemKind::Property, &ids)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;
        } else {
            vis.unhide_to_batch(batch, branch_id, tx_id, ItemKind::Property, &ids)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;
        }
    }

    Ok(())
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
    registry: &BranchSchemaRegistry,
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
    use crate::assertion::{PropertyValue, RangeValue, SetValue, MAIN_BRANCH};
    use crate::schema::ValueType;
    use serde_json::json;

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
        introduce: Vec<super::super::IntroduceItem>,
        asserts: Vec<super::super::AssertItem>,
    ) -> TransactionInput {
        TransactionInput {
            reasoning,
            entity_types: vec![],
            properties: vec![],
            attach: vec![],
            hide: super::super::HideUnhideInput::default(),
            unhide: super::super::HideUnhideInput::default(),
            introduce,
            asserts,
        }
    }

    #[test]
    fn create_entity_with_facts() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = execute_transaction(
            tx_input(
                json!("initial analysis"),
                vec![super::super::IntroduceItem {
                    entity: "song-1".into(),
                    description: Some("A great song".into()),
                    facts: vec![super::super::AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(128))),
                        )]),
                        reasoning: json!("detected from waveform"),
                    }],
                    hypotheses: vec![],
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.introduced.len(), 1);
        assert_eq!(result.introduced[0].facts.len(), 1);
        assert!(result.introduced[0].hypotheses.is_empty());

        let entity = store.entities().get_by_slug("song-1").unwrap().unwrap();
        assert_eq!(entity.description.as_deref(), Some("A great song"));

        let log = store.facts().list().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].tx_id, result.transaction.id);
    }

    #[test]
    fn create_entity_with_hypotheses() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = execute_transaction(
            tx_input(
                json!("exploring possibilities"),
                vec![super::super::IntroduceItem {
                    entity: "song-2".into(),
                    description: None,
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
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        assert!(result.introduced[0].facts.is_empty());
        assert_eq!(result.introduced[0].hypotheses.len(), 2);

        let hyp_log = store.hypotheses().list().unwrap();
        assert_eq!(hyp_log.len(), 2);
    }

    #[test]
    fn assert_entity_existing() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        store.entities().create("song-3", None).unwrap();

        let result = execute_transaction(
            tx_input(
                json!("follow-up analysis"),
                vec![],
                vec![super::super::AssertItem {
                    entity: "song-3".into(),
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
                }],
            ),
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.asserted.len(), 1);
        assert_eq!(result.asserted[0].facts.len(), 1);
    }

    #[test]
    fn assert_entity_not_found() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![],
                vec![super::super::AssertItem {
                    entity: "nonexistent".into(),
                    facts: vec![],
                    hypotheses: vec![],
                }],
            ),
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

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "dupe".into(),
                    description: None,
                    facts: vec![],
                    hypotheses: vec![],
                }],
                vec![],
            ),
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

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "conflict".into(),
                    description: None,
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
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::ConflictingFacts { .. }));
        assert!(store.entities().get_by_slug("conflict").unwrap().is_none());
    }

    #[test]
    fn unknown_entity_type_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "bad-type".into(),
                    description: None,
                    facts: vec![super::super::AssertionItem {
                        entity_type: "nonexistent-type".into(),
                        properties: HashMap::new(),
                        reasoning: json!(null),
                    }],
                    hypotheses: vec![],
                }],
                vec![],
            ),
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

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "bad-prop".into(),
                    description: None,
                    facts: vec![super::super::AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "nonexistent-prop".into(),
                            PropertyValue::Range(RangeValue::Eq(json!(0))),
                        )]),
                        reasoning: json!(null),
                    }],
                    hypotheses: vec![],
                }],
                vec![],
            ),
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

        let result = execute_transaction(
            tx_input(
                json!("comprehensive analysis"),
                vec![super::super::IntroduceItem {
                    entity: "mixed".into(),
                    description: Some("Mixed assertions".into()),
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
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.introduced[0].facts.len(), 1);
        assert_eq!(result.introduced[0].hypotheses.len(), 2);

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

    #[test]
    fn transaction_introduce_multiple_entities() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = execute_transaction(
            tx_input(
                json!("catalog import"),
                vec![
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
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.transaction.branch_id, crate::assertion::MAIN_BRANCH);
        assert_eq!(result.introduced.len(), 2);
        assert_eq!(result.asserted.len(), 0);
        assert_eq!(result.introduced[0].entity_slug, "song-a");
        assert_eq!(result.introduced[1].entity_slug, "song-b");
        assert_eq!(result.introduced[0].facts.len(), 1);
        assert_eq!(result.introduced[1].facts.len(), 1);

        let tx_id = result.transaction.id;
        assert!(result.introduced[0].facts[0].tx_id == tx_id);
        assert!(result.introduced[1].facts[0].tx_id == tx_id);

        assert!(store.entities().get_by_slug("song-a").unwrap().is_some());
        assert!(store.entities().get_by_slug("song-b").unwrap().is_some());
    }

    #[test]
    fn transaction_assert_on_existing_entity() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        store.entities().create("existing-song", None).unwrap();

        let result = execute_transaction(
            tx_input(
                json!("follow-up"),
                vec![],
                vec![super::super::AssertItem {
                    entity: "existing-song".into(),
                    facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(100))))],
                    hypotheses: vec![],
                }],
            ),
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
            tx_input(
                json!("create and annotate"),
                vec![super::super::IntroduceItem {
                    entity: "new-song".into(),
                    description: Some("Brand new".into()),
                    facts: vec![],
                    hypotheses: vec![],
                }],
                vec![super::super::AssertItem {
                    entity: "new-song".into(),
                    facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(130))))],
                    hypotheses: vec![],
                }],
            ),
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.introduced.len(), 1);
        assert_eq!(result.asserted.len(), 1);
        assert_eq!(result.introduced[0].entity_id, result.asserted[0].entity_id);
    }

    #[test]
    fn transaction_duplicate_introduce_slugs_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![
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
                vec![],
            ),
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
            tx_input(
                json!(null),
                vec![],
                vec![super::super::AssertItem {
                    entity: "ghost".into(),
                    facts: vec![],
                    hypotheses: vec![],
                }],
            ),
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
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "conflict".into(),
                    description: None,
                    facts: vec![
                        make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(120)))),
                        make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(130)))),
                    ],
                    hypotheses: vec![],
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::ConflictingFacts { .. }));
        assert!(store.entities().get_by_slug("conflict").unwrap().is_none());
    }

    #[test]
    fn transaction_introduce_already_exists_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        store.entities().create("taken", None).unwrap();

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "taken".into(),
                    description: None,
                    facts: vec![],
                    hypotheses: vec![],
                }],
                vec![],
            ),
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
            tx_input(
                json!("comprehensive import"),
                vec![super::super::IntroduceItem {
                    entity: "song-x".into(),
                    description: Some("A song".into()),
                    facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(128))))],
                    hypotheses: vec![make_item("track", "certification", PropertyValue::Set(SetValue {
                        contains: vec![json!("gold")],
                        not_contains: vec![],
                    }))],
                }],
                vec![super::super::AssertItem {
                    entity: "song-x".into(),
                    facts: vec![],
                    hypotheses: vec![make_item("track", "certification", PropertyValue::Set(SetValue {
                        contains: vec![json!("platinum")],
                        not_contains: vec![],
                    }))],
                }],
            ),
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.introduced[0].facts.len(), 1);
        assert_eq!(result.introduced[0].hypotheses.len(), 1);
        assert_eq!(result.asserted[0].facts.len(), 0);
        assert_eq!(result.asserted[0].hypotheses.len(), 1);

        let tx_id = result.transaction.id;
        for entry in &result.introduced[0].facts {
            assert_eq!(entry.tx_id, tx_id);
        }
        for entry in &result.asserted[0].hypotheses {
            assert_eq!(entry.tx_id, tx_id);
        }
    }

    #[test]
    fn same_property_in_facts_and_hypotheses_allowed() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let result = execute_transaction(
            tx_input(
                json!("mixed certainty"),
                vec![super::super::IntroduceItem {
                    entity: "overlap".into(),
                    description: None,
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
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.introduced[0].facts.len(), 1);
        assert_eq!(result.introduced[0].hypotheses.len(), 1);
    }

    #[test]
    fn transaction_with_schema_operations() {
        let (reg, store, _dir) = setup();

        let result = execute_transaction(
            TransactionInput {
                reasoning: json!("bootstrap schema and data"),
                entity_types: vec![super::super::CreateEntityType {
                    slug: "track".into(),
                    description: None,
                    properties: vec![],
                }],
                properties: vec![super::super::CreateProperty {
                    slug: "bpm".into(),
                    value_type: ValueType::Range,
                    description: None,
                    entity_types: vec![],
                }],
                attach: vec![super::super::AttachProperty {
                    entity_type: "track".into(),
                    property: "bpm".into(),
                }],
                hide: super::super::HideUnhideInput::default(),
                unhide: super::super::HideUnhideInput::default(),
                introduce: vec![super::super::IntroduceItem {
                    entity: "song-1".into(),
                    description: None,
                    facts: vec![make_item("track", "bpm", PropertyValue::Range(RangeValue::Eq(json!(128))))],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.entity_types.len(), 1);
        assert_eq!(result.entity_types[0].slug, "track");
        assert_eq!(result.properties.len(), 1);
        assert_eq!(result.properties[0].slug, "bpm");
        assert_eq!(result.attached_count, 1);
        assert_eq!(result.introduced.len(), 1);
        assert_eq!(result.introduced[0].facts.len(), 1);
    }
}
