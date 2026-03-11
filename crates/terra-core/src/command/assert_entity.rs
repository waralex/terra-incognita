use std::collections::HashMap;

use uuid::Uuid;

use crate::assertion::{
    AssertionInput, AssertionStore, EntityError, ItemKind, Transaction,
    WriterError,
};
use crate::schema::{AddPropertiesInput, BranchSchemaRegistry, EntityProperty, EntityType, EntityTypeInput, PropertyDef};

use super::{TransactionEntityResult, TransactionInput};

/// Errors specific to the transaction business logic.
#[derive(Debug, thiserror::Error)]
pub enum AssertEntityError {
    /// Empty transaction without reasoning.
    #[error("empty transaction: if no data changes are needed, reasoning is required to explain why")]
    EmptyTransaction,

    /// Entity must exist for assertion but was not found.
    #[error("entity not found: {0}")]
    EntityNotFound(String),

    /// Entity already exists (during creation).
    #[error("entity already exists: {0}")]
    EntityAlreadyExists(String),

    /// Entity exists but is hidden on this branch.
    #[error("entity \"{0}\" exists but is hidden on this branch — use unhide to bring it into scope")]
    EntityHidden(String),

    /// Two facts in the same transaction assert the same property.
    #[error(
        "conflicting facts: property \"{property}\" \
         is asserted more than once in the same transaction — \
         if values are uncertain, use hypotheses instead of facts"
    )]
    ConflictingFacts {
        property: String,
    },

    /// Referenced entity type not found in schema.
    #[error("entity type not found: {0}")]
    EntityTypeNotFound(String),

    /// Entity type exists but is hidden on this branch.
    #[error("entity type \"{0}\" exists but is hidden on this branch — use unhide to bring it into scope")]
    EntityTypeHidden(String),

    /// Referenced property not found on entity's type.
    #[error("property \"{0}\" not found on entity type")]
    PropertyNotFound(String),

    /// Property exists but is hidden on this branch.
    #[error("property \"{0}\" exists but is hidden on this branch — use unhide to bring it into scope")]
    PropertyHidden(String),

    /// Entity type mismatch — entity belongs to a different type.
    #[error("entity \"{entity}\" belongs to type \"{actual_type}\", not \"{expected_type}\"")]
    EntityTypeMismatch {
        entity: String,
        actual_type: String,
        expected_type: String,
    },

    /// Introduce item is missing entity_type.
    #[error("introduce item \"{0}\" is missing entity_type")]
    MissingEntityType(String),

    /// Task not found by slug.
    #[error("task not found: {0}")]
    TaskNotFound(String),

    /// Task already exists (slug taken).
    #[error("task already exists: {0}")]
    TaskAlreadyExists(String),

    /// Task exists but is hidden on this branch.
    #[error("task \"{0}\" exists but is hidden on this branch — use unhide to bring it into scope")]
    TaskHidden(String),

    /// Task is already closed.
    #[error("task \"{0}\" is already closed")]
    TaskAlreadyClosed(String),

    /// Entity storage error.
    #[error(transparent)]
    Entity(#[from] EntityError),

    /// Task storage error.
    #[error(transparent)]
    Task(#[from] crate::assertion::TaskError),

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
    pub introduced: Vec<TransactionEntityResult>,
    pub asserted: Vec<TransactionEntityResult>,
}

/// Executes a unified transaction: schema operations, visibility changes,
/// entity introduction, and assertions — all in one command.
///
/// Processing order:
/// 1. Create entity types with inline properties
/// 2. Add properties to existing types
/// 3. Hide/unhide visibility changes
/// 4. Introduce new entities (bound to entity type)
/// 5. Write assertions on introduced and existing entities
pub fn execute_transaction(
    input: TransactionInput,
    registry: &BranchSchemaRegistry,
    store: &AssertionStore,
) -> Result<TransactionExecResult, AssertEntityError> {
    // Validate: empty transactions (no mutations) require reasoning.
    let is_empty = input.entity_types.is_empty()
        && input.add_properties.is_empty()
        && input.hide.entities.is_empty()
        && input.hide.entity_types.is_empty()
        && input.hide.properties.is_empty()
        && input.unhide.entities.is_empty()
        && input.unhide.entity_types.is_empty()
        && input.unhide.properties.is_empty()
        && input.introduce.is_empty()
        && input.asserts.is_empty()
        && input.tasks.is_empty()
        && input.update_tasks.is_empty()
        && input.close_tasks.is_empty();
    if is_empty && (input.reasoning.is_null() || input.reasoning == serde_json::Value::String(String::new())) {
        return Err(AssertEntityError::EmptyTransaction);
    }

    // Phase 0: Schema operations
    // Create entity types with inline properties
    let created_entity_types = if input.entity_types.is_empty() {
        vec![]
    } else {
        let prop_defs: Vec<Vec<PropertyDef<'_>>> = input
            .entity_types
            .iter()
            .map(|item| {
                item.properties
                    .iter()
                    .map(|p| PropertyDef {
                        slug: &p.slug,
                        value_type: p.value_type,
                        description: p.description.as_deref(),
                    })
                    .collect()
            })
            .collect();
        let inputs: Vec<EntityTypeInput<'_>> = input
            .entity_types
            .iter()
            .zip(prop_defs.iter())
            .map(|(item, props)| EntityTypeInput {
                slug: &item.slug,
                description: item.description.as_deref(),
                properties: props,
            })
            .collect();
        registry.create_entity_types_batch(&inputs)?
    };

    // Add properties to existing types
    let created_properties = if input.add_properties.is_empty() {
        vec![]
    } else {
        let prop_defs: Vec<Vec<PropertyDef<'_>>> = input
            .add_properties
            .iter()
            .map(|item| {
                item.properties
                    .iter()
                    .map(|p| PropertyDef {
                        slug: &p.slug,
                        value_type: p.value_type,
                        description: p.description.as_deref(),
                    })
                    .collect()
            })
            .collect();
        let inputs: Vec<AddPropertiesInput<'_>> = input
            .add_properties
            .iter()
            .zip(prop_defs.iter())
            .map(|(item, props)| AddPropertiesInput {
                entity_type: &item.entity_type,
                properties: props,
            })
            .collect();
        registry.add_properties_to_type(&inputs)?
    };

    // Phase 1: Validate assertions before any entity mutation

    let entities = store.entities(registry.branch_id(), registry.ancestry().to_vec());
    let vis = store.visibility();
    let ancestry = registry.ancestry();

    // For introduces: validate entity_type, resolve properties against that type
    let mut intro_resolved: Vec<(String, String, Option<String>, Vec<AssertionInput>, Vec<AssertionInput>)> =
        Vec::with_capacity(input.introduce.len());
    for item in &input.introduce {
        // Validate entity_type
        let entity_type = registry.get_entity_type(&item.entity_type).map_err(|e| {
            match e {
                crate::schema::SchemaError::EntityTypeNotFound(_) => {
                    AssertEntityError::EntityTypeNotFound(item.entity_type.clone())
                }
                other => AssertEntityError::Schema(other),
            }
        })?;

        if !vis.is_visible(ancestry, ItemKind::EntityType, entity_type.id)
            .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))? {
            return Err(AssertEntityError::EntityTypeHidden(item.entity_type.clone()));
        }

        validate_no_conflicting_facts(&item.facts)?;
        let facts = resolve_items(&item.facts, Uuid::nil(), entity_type.id, registry, &vis)?;
        let hyps = resolve_items(&item.hypotheses, Uuid::nil(), entity_type.id, registry, &vis)?;

        if let Some(record) = entities.get_by_slug(&item.entity)? {
            if !vis.is_visible(ancestry, ItemKind::Entity, record.id)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))? {
                return Err(AssertEntityError::EntityHidden(item.entity.clone()));
            }
            return Err(AssertEntityError::EntityAlreadyExists(item.entity.clone()));
        }
        intro_resolved.push((
            item.entity.clone(),
            item.entity_type.clone(),
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

    // For asserts: look up entity's type from its record, resolve properties against that type
    let mut assert_resolved: Vec<(Uuid, String, Vec<AssertionInput>, Vec<AssertionInput>)> =
        Vec::with_capacity(input.asserts.len());
    for item in &input.asserts {
        validate_no_conflicting_facts(&item.facts)?;

        if let Some(record) = entities.get_by_slug(&item.entity)? {
            if !vis.is_visible(ancestry, ItemKind::Entity, record.id)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))? {
                return Err(AssertEntityError::EntityHidden(item.entity.clone()));
            }

            let entity_type_id = record.entity_type_id.ok_or_else(|| {
                AssertEntityError::EntityTypeNotFound(format!(
                    "entity \"{}\" has no bound entity type (legacy entity)", item.entity
                ))
            })?;

            let facts = resolve_items(&item.facts, record.id, entity_type_id, registry, &vis)?;
            let hyps = resolve_items(&item.hypotheses, record.id, entity_type_id, registry, &vis)?;
            assert_resolved.push((record.id, item.entity.clone(), facts, hyps));
        } else if let Some(intro) = intro_resolved.iter().find(|(slug, _, _, _, _)| slug == &item.entity) {
            // References an introduced entity — use the entity type from the introduce
            let et = registry.get_entity_type(&intro.1)?;
            let facts = resolve_items(&item.facts, Uuid::nil(), et.id, registry, &vis)?;
            let hyps = resolve_items(&item.hypotheses, Uuid::nil(), et.id, registry, &vis)?;
            assert_resolved.push((Uuid::nil(), item.entity.clone(), facts, hyps));
        } else {
            return Err(AssertEntityError::EntityNotFound(item.entity.clone()));
        }
    }

    // Phase 2: Create entities from introduce list
    let mut intro_entities: Vec<(Uuid, String)> = Vec::with_capacity(intro_resolved.len());
    for (slug, entity_type_slug, desc, _, _) in &intro_resolved {
        let et = registry.get_entity_type(entity_type_slug)?;
        let record = entities.create(slug, desc.as_deref(), et.id)?;
        intro_entities.push((record.id, slug.clone()));
    }

    // Fix entity_ids for introduce resolved items
    for ((entity_id, _), (_, _, _, facts, hyps)) in intro_entities.iter().zip(intro_resolved.iter_mut())
    {
        for item in facts.iter_mut().chain(hyps.iter_mut()) {
            item.entity_id = *entity_id;
        }
    }

    // Fix entity_ids for all assert resolved items.
    for (entity_id, slug, facts, hyps) in assert_resolved.iter_mut() {
        if *entity_id == Uuid::nil() {
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
        branch_id: registry.branch_id(),
        reasoning: input.reasoning,
        question: input.question,
        answer: input.answer,
        commands: input.commands,
        timestamp: chrono::Utc::now(),
    };

    let mut batch = rocksdb::WriteBatch::default();
    store
        .transactions()
        .put_to_batch(&mut batch, &tx)
        .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;

    // Visibility: hide
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

    // Task visibility: hide
    {
        let task_store = store.tasks(registry.branch_id(), registry.ancestry().to_vec());
        if !input.hide.tasks.is_empty() {
            let mut ids = Vec::with_capacity(input.hide.tasks.len());
            for slug in &input.hide.tasks {
                let record = task_store
                    .get_by_slug(slug)
                    .map_err(AssertEntityError::Task)?
                    .ok_or_else(|| AssertEntityError::TaskNotFound(slug.clone()))?;
                ids.push(record.id);
            }
            vis.hide_to_batch(&mut batch, tx.branch_id, tx.id, ItemKind::Task, &ids)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;
        }
        if !input.unhide.tasks.is_empty() {
            let mut ids = Vec::with_capacity(input.unhide.tasks.len());
            for slug in &input.unhide.tasks {
                let record = task_store
                    .get_by_slug(slug)
                    .map_err(AssertEntityError::Task)?
                    .ok_or_else(|| AssertEntityError::TaskNotFound(slug.clone()))?;
                ids.push(record.id);
            }
            vis.unhide_to_batch(&mut batch, tx.branch_id, tx.id, ItemKind::Task, &ids)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))?;
        }

        // Create new tasks
        for item in &input.tasks {
            if task_store.get_by_slug(&item.slug).map_err(AssertEntityError::Task)?.is_some() {
                return Err(AssertEntityError::TaskAlreadyExists(item.slug.clone()));
            }
            task_store.create(&item.slug, item.goal.clone(), &item.reasoning, item.context.clone(), item.kind.as_deref(), tx.id)
                .map_err(AssertEntityError::Task)?;
        }

        // Update task notes
        for item in &input.update_tasks {
            let record = task_store
                .get_by_slug(&item.slug)
                .map_err(AssertEntityError::Task)?
                .ok_or_else(|| AssertEntityError::TaskNotFound(item.slug.clone()))?;
            if !vis.is_visible(ancestry, ItemKind::Task, record.id)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))? {
                return Err(AssertEntityError::TaskHidden(item.slug.clone()));
            }
            if record.status == crate::assertion::TaskStatus::Closed {
                return Err(AssertEntityError::TaskAlreadyClosed(item.slug.clone()));
            }
            task_store.update_notes(&record.id, item.notes.clone(), tx.id)
                .map_err(AssertEntityError::Task)?;
        }

        // Close tasks
        for item in &input.close_tasks {
            let record = task_store
                .get_by_slug(&item.slug)
                .map_err(AssertEntityError::Task)?
                .ok_or_else(|| AssertEntityError::TaskNotFound(item.slug.clone()))?;
            if !vis.is_visible(ancestry, ItemKind::Task, record.id)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))? {
                return Err(AssertEntityError::TaskHidden(item.slug.clone()));
            }
            if record.status == crate::assertion::TaskStatus::Closed {
                return Err(AssertEntityError::TaskAlreadyClosed(item.slug.clone()));
            }
            task_store.close(&record.id, item.resolution.clone(), tx.id)
                .map_err(AssertEntityError::Task)?;
        }
    }

    let fact_writer = store.fact_writer();
    let hyp_writer = store.hypothesis_writer();

    // Write introduces
    let mut introduced = Vec::with_capacity(intro_resolved.len());
    for ((entity_id, slug), (_, _, _, facts, hyps)) in
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
                if prev_item.properties.contains_key(property_slug) {
                    return Err(AssertEntityError::ConflictingFacts {
                        property: property_slug.clone(),
                    });
                }
            }
        }
    }

    Ok(())
}

/// Resolves slug-based AssertionItems into UUID-based AssertionInputs.
/// entity_type_id is derived from the entity's bound type (not from the assertion).
fn resolve_items(
    items: &[super::AssertionItem],
    entity_id: Uuid,
    entity_type_id: Uuid,
    registry: &BranchSchemaRegistry,
    vis: &crate::assertion::VisibilityStore,
) -> Result<Vec<AssertionInput>, AssertEntityError> {
    let ancestry = registry.ancestry();
    let mut result = Vec::with_capacity(items.len());

    let attached_props = registry.list_properties_by_type_id(&entity_type_id)?;
    let prop_map: HashMap<&str, (Uuid, crate::schema::ValueType)> = attached_props
        .iter()
        .map(|p| (p.slug.as_str(), (p.id, p.value_type)))
        .collect();

    for item in items {
        let mut properties = HashMap::with_capacity(item.properties.len());
        for (slug, value) in &item.properties {
            let (prop_id, _vt) = prop_map.get(slug.as_str()).ok_or_else(|| {
                if let Ok(prop) = registry.get_property_by_slug(slug) {
                    if !vis.is_visible(ancestry, ItemKind::Property, prop.id).unwrap_or(true) {
                        return AssertEntityError::PropertyHidden(slug.clone());
                    }
                }
                AssertEntityError::PropertyNotFound(slug.clone())
            })?;

            if !vis.is_visible(ancestry, ItemKind::Property, *prop_id)
                .map_err(|e| AssertEntityError::Writer(WriterError::Storage(e)))? {
                return Err(AssertEntityError::PropertyHidden(slug.clone()));
            }

            properties.insert(*prop_id, value.clone());
        }

        result.push(AssertionInput {
            entity_id,
            entity_type_id,
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
        use crate::schema::{EntityTypeInput, PropertyDef};
        reg.create_entity_types_batch(&[EntityTypeInput {
            slug: "track",
            description: None,
            properties: &[
                PropertyDef { slug: "bpm", value_type: ValueType::Range, description: None },
                PropertyDef { slug: "certification", value_type: ValueType::Set, description: None },
            ],
        }]).unwrap();
    }

    fn tx_input(
        reasoning: serde_json::Value,
        introduce: Vec<super::super::IntroduceItem>,
        asserts: Vec<super::super::AssertItem>,
    ) -> TransactionInput {
        TransactionInput {
            reasoning,
            question: None,
            answer: None,
            commands: vec![],
            entity_types: vec![],
            add_properties: vec![],
            hide: super::super::HideUnhideInput::default(),
            unhide: super::super::HideUnhideInput::default(),
            introduce,
            asserts,
            tasks: vec![],
            update_tasks: vec![],
            close_tasks: vec![],
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
                    entity_type: "track".into(),
                    description: Some("A great song".into()),
                    facts: vec![super::super::AssertionItem {
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

        let entity = store.entities(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]).get_by_slug("song-1").unwrap().unwrap();
        assert_eq!(entity.description.as_deref(), Some("A great song"));
        assert!(entity.entity_type_id.is_some());
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
                    entity_type: "track".into(),
                    description: None,
                    facts: vec![],
                    hypotheses: vec![
                        super::super::AssertionItem {
                            properties: HashMap::from([(
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(json!(120))),
                            )]),
                            reasoning: json!("estimate A"),
                        },
                        super::super::AssertionItem {
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
    }

    #[test]
    fn assert_entity_existing() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let et = reg.get_entity_type("track").unwrap();
        store.entities(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]).create("song-3", None, et.id).unwrap();

        let result = execute_transaction(
            tx_input(
                json!("follow-up analysis"),
                vec![],
                vec![super::super::AssertItem {
                    entity: "song-3".into(),
                    facts: vec![super::super::AssertionItem {
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
    fn conflicting_facts_rejected() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "conflict".into(),
                    entity_type: "track".into(),
                    description: None,
                    facts: vec![
                        super::super::AssertionItem {
                            properties: HashMap::from([(
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(json!(120))),
                            )]),
                            reasoning: json!("analysis A"),
                        },
                        super::super::AssertionItem {
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
                    entity_type: "track".into(),
                    description: None,
                    facts: vec![super::super::AssertionItem {
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

        assert!(matches!(err, AssertEntityError::PropertyNotFound(_)));
    }

    #[test]
    fn transaction_with_inline_schema() {
        let (reg, store, _dir) = setup();

        let result = execute_transaction(
            TransactionInput {
                reasoning: json!("bootstrap schema and data"),
                question: None,
                answer: None,
                commands: vec![],
                entity_types: vec![super::super::CreateEntityType {
                    slug: "track".into(),
                    description: None,
                    properties: vec![super::super::CreatePropertyDef {
                        slug: "bpm".into(),
                        value_type: ValueType::Range,
                        description: None,
                    }],
                }],
                add_properties: vec![],
                hide: super::super::HideUnhideInput::default(),
                unhide: super::super::HideUnhideInput::default(),
                introduce: vec![super::super::IntroduceItem {
                    entity: "song-1".into(),
                    entity_type: "track".into(),
                    description: None,
                    facts: vec![super::super::AssertionItem {
                        properties: HashMap::from([("bpm".into(), PropertyValue::Range(RangeValue::Eq(json!(128))))]),
                        reasoning: json!("detected"),
                    }],
                    hypotheses: vec![],
                }],
                asserts: vec![],
                tasks: vec![],
                update_tasks: vec![],
                close_tasks: vec![],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(result.entity_types.len(), 1);
        assert_eq!(result.introduced.len(), 1);
        assert_eq!(result.introduced[0].facts.len(), 1);
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
                    entity_type: "track".into(),
                    description: Some("Brand new".into()),
                    facts: vec![],
                    hypotheses: vec![],
                }],
                vec![super::super::AssertItem {
                    entity: "new-song".into(),
                    facts: vec![super::super::AssertionItem {
                        properties: HashMap::from([("bpm".into(), PropertyValue::Range(RangeValue::Eq(json!(130))))]),
                        reasoning: json!("detected"),
                    }],
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

    fn make_item(prop: &str, value: PropertyValue) -> super::super::AssertionItem {
        super::super::AssertionItem {
            properties: HashMap::from([(prop.into(), value)]),
            reasoning: json!(null),
        }
    }

    #[test]
    fn hidden_entity_type_rejected_in_assertion() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let tx = Transaction {
            id: Uuid::now_v7(),
            branch_id: MAIN_BRANCH,
            reasoning: json!("hide track"),
            question: None,
            answer: None,
            commands: vec![],
            timestamp: chrono::Utc::now(),
        };
        let mut batch = rocksdb::WriteBatch::default();
        store.transactions().put_to_batch(&mut batch, &tx).unwrap();
        let et = reg.get_entity_type("track").unwrap();
        store.visibility().hide_to_batch(
            &mut batch, MAIN_BRANCH, tx.id, ItemKind::EntityType, &[et.id],
        ).unwrap();
        store.write_batch(batch).unwrap();

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "song".into(),
                    entity_type: "track".into(),
                    description: None,
                    facts: vec![make_item("bpm", PropertyValue::Range(RangeValue::Eq(json!(120))))],
                    hypotheses: vec![],
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::EntityTypeHidden(_)));
    }

    #[test]
    fn hidden_property_rejected_in_assertion() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg);

        let tx = Transaction {
            id: Uuid::now_v7(),
            branch_id: MAIN_BRANCH,
            reasoning: json!("hide bpm"),
            question: None,
            answer: None,
            commands: vec![],
            timestamp: chrono::Utc::now(),
        };
        let mut batch = rocksdb::WriteBatch::default();
        store.transactions().put_to_batch(&mut batch, &tx).unwrap();
        let prop = reg.get_property_by_slug("bpm").unwrap();
        store.visibility().hide_to_batch(
            &mut batch, MAIN_BRANCH, tx.id, ItemKind::Property, &[prop.id],
        ).unwrap();
        store.write_batch(batch).unwrap();

        let err = execute_transaction(
            tx_input(
                json!(null),
                vec![super::super::IntroduceItem {
                    entity: "song".into(),
                    entity_type: "track".into(),
                    description: None,
                    facts: vec![make_item("bpm", PropertyValue::Range(RangeValue::Eq(json!(120))))],
                    hypotheses: vec![],
                }],
                vec![],
            ),
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, AssertEntityError::PropertyHidden(_)));
    }
}
