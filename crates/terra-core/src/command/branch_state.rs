use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::assertion::{AssertionStore, ColumnCell, ItemKind, MAIN_BRANCH};
use crate::schema::{BranchSchemaRegistry, EntityProperty, ValueType};

/// Full epistemic state of a branch — schema, entities, assertions, recent transactions.
#[derive(Debug, Serialize)]
pub struct BranchState {
    pub branch: BranchInfo,
    pub schema: SchemaSnapshot,
    pub entities: Vec<EntityState>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub investigations: Vec<InvestigationSnapshot>,
    pub recent_transactions: Vec<TransactionSnapshot>,
}

/// Branch identity and reasoning.
#[derive(Debug, Serialize)]
pub struct BranchInfo {
    pub id: Uuid,
    pub slug: String,
    pub reasoning: serde_json::Value,
}

/// All visible schema items on the branch.
#[derive(Debug, Serialize)]
pub struct SchemaSnapshot {
    pub entity_types: Vec<EntityTypeSnapshot>,
    pub properties: Vec<PropertySnapshot>,
}

/// Entity type with attached property slugs.
#[derive(Debug, Serialize)]
pub struct EntityTypeSnapshot {
    pub id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub properties: Vec<String>,
}

/// Property definition.
#[derive(Debug, Serialize)]
pub struct PropertySnapshot {
    pub id: Uuid,
    pub slug: String,
    pub value_type: ValueType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Full state of an entity across all entity types with assertions.
#[derive(Debug, Serialize)]
pub struct EntityState {
    pub id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub types: Vec<EntityTypeState>,
}

/// Entity's assertions grouped by entity type.
#[derive(Debug, Serialize)]
pub struct EntityTypeState {
    pub entity_type: String,
    pub properties: Vec<PropertyFullState>,
}

/// Full property state: latest fact + pending hypotheses with values and reasoning.
#[derive(Debug, Serialize)]
pub struct PropertyFullState {
    pub slug: String,
    pub value_type: ValueType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fact: Option<FactSnapshot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hypotheses: Vec<HypothesisSnapshot>,
}

/// A convergence point — the latest decided value.
#[derive(Debug, Serialize)]
pub struct FactSnapshot {
    pub value: serde_json::Value,
    pub reasoning: serde_json::Value,
    pub tx_id: Uuid,
}

/// A tentative claim under consideration.
#[derive(Debug, Serialize)]
pub struct HypothesisSnapshot {
    pub value: serde_json::Value,
    pub reasoning: serde_json::Value,
    pub tx_id: Uuid,
}

/// Recent transaction with reasoning, question, and answer.
#[derive(Debug, Serialize)]
pub struct TransactionSnapshot {
    pub id: Uuid,
    pub reasoning: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<serde_json::Value>,
    pub timestamp: DateTime<Utc>,
}

/// Open investigation in the branch state.
#[derive(Debug, Serialize)]
pub struct InvestigationSnapshot {
    pub id: Uuid,
    pub slug: String,
    pub goal: serde_json::Value,
    pub reasoning: String,
    pub context: serde_json::Value,
    #[serde(skip_serializing_if = "serde_json::Value::is_null")]
    pub notes: serde_json::Value,
    pub tx_id: Uuid,
}

/// Errors from building branch state.
#[derive(Debug, thiserror::Error)]
pub enum BranchStateError {
    #[error("branch not found: {0}")]
    BranchNotFound(String),

    #[error(transparent)]
    Schema(#[from] crate::schema::SchemaError),

    #[error(transparent)]
    Storage(#[from] crate::assertion::LogError),

    #[error(transparent)]
    Entity(#[from] crate::assertion::EntityError),

    #[error(transparent)]
    Branch(#[from] crate::assertion::BranchError),

    #[error(transparent)]
    Investigation(#[from] crate::assertion::InvestigationError),
}

/// Builds a complete branch state snapshot.
///
/// When `at_tx` is `Some`, returns state as of that transaction (time travel).
/// When `None`, returns current HEAD state.
pub fn build_state(
    slug: &str,
    last_transactions: usize,
    at_tx: Option<Uuid>,
    registry: &BranchSchemaRegistry,
    store: &AssertionStore,
) -> Result<BranchState, BranchStateError> {
    let branch_info = resolve_branch(slug, store)?;
    let branch_id = branch_info.id;
    let bound = at_tx.unwrap_or(Uuid::max());

    // Cap ancestry so schema registry and visibility only see items up to bound
    let capped = cap_ancestry(registry.ancestry(), bound);
    let schema_reg = store.schema_registry(registry.branch_id(), capped.clone());

    let vis = store.visibility();

    // 2. Schema snapshot
    let all_types = schema_reg.list_entity_types()?;
    let visible_types: Vec<_> = all_types
        .into_iter()
        .filter(|t| vis.is_visible(&capped, ItemKind::EntityType, t.id).unwrap_or(true))
        .collect();

    let all_props = schema_reg.list_all_properties()?;
    let visible_props: Vec<_> = all_props
        .into_iter()
        .filter(|p| vis.is_visible(&capped, ItemKind::Property, p.id).unwrap_or(true))
        .collect();

    let mut entity_type_snapshots = Vec::new();
    for et in &visible_types {
        let attached = schema_reg.list_properties_by_type_id(&et.id)?;
        let prop_slugs: Vec<String> = attached
            .iter()
            .filter(|p| vis.is_visible(&capped, ItemKind::Property, p.id).unwrap_or(true))
            .map(|p| p.slug.clone())
            .collect();
        entity_type_snapshots.push(EntityTypeSnapshot {
            id: et.id,
            slug: et.slug.clone(),
            description: et.description.clone(),
            properties: prop_slugs,
        });
    }

    let property_snapshots: Vec<PropertySnapshot> = visible_props
        .iter()
        .map(|p| PropertySnapshot {
            id: p.id,
            slug: p.slug.clone(),
            value_type: p.value_type,
            description: p.description.clone(),
        })
        .collect();

    let schema = SchemaSnapshot {
        entity_types: entity_type_snapshots,
        properties: property_snapshots,
    };

    // 3. Entity states
    let entities = store.entities(registry.branch_id(), capped.clone()).list_active_at(bound)?;
    let visible_entities: Vec<_> = entities
        .into_iter()
        .filter(|e| vis.is_visible(&capped, ItemKind::Entity, e.id).unwrap_or(true))
        .collect();

    let mut entity_states = Vec::new();
    for entity in &visible_entities {
        let mut type_states = Vec::new();

        for et in &visible_types {
            let attached = schema_reg.list_properties_by_type_id(&et.id)?;
            let visible_attached: Vec<_> = attached
                .into_iter()
                .filter(|p| vis.is_visible(&capped, ItemKind::Property, p.id).unwrap_or(true))
                .collect();

            let mut prop_states = Vec::new();
            let mut has_any_data = false;

            for prop in &visible_attached {
                let pfs = resolve_property_full_state(entity.id, prop, bound, store)?;
                if pfs.fact.is_some() || !pfs.hypotheses.is_empty() {
                    has_any_data = true;
                }
                prop_states.push(pfs);
            }

            if has_any_data {
                type_states.push(EntityTypeState {
                    entity_type: et.slug.clone(),
                    properties: prop_states,
                });
            }
        }

        if !type_states.is_empty() {
            entity_states.push(EntityState {
                id: entity.id,
                slug: entity.slug.clone(),
                description: entity.description.clone(),
                types: type_states,
            });
        }
    }

    // 4. Open investigations
    let inv_store = store.investigations(registry.branch_id(), capped.clone());
    let all_investigations = inv_store.list_open_at(bound)?;
    let investigations: Vec<InvestigationSnapshot> = all_investigations
        .into_iter()
        .filter(|inv| vis.is_visible(&capped, ItemKind::Investigation, inv.id).unwrap_or(true))
        .map(|inv| InvestigationSnapshot {
            id: inv.id,
            slug: inv.slug,
            goal: inv.goal,
            reasoning: inv.reasoning,
            context: inv.context,
            notes: inv.notes,
            tx_id: inv.tx_id,
        })
        .collect();

    // 5. Recent transactions
    let mut txns = store.transactions().list_by_branch_at(&branch_id, &bound)?;
    txns.reverse(); // newest first
    txns.truncate(last_transactions);

    let recent_transactions = txns
        .into_iter()
        .map(|tx| TransactionSnapshot {
            id: tx.id,
            reasoning: tx.reasoning,
            question: tx.question,
            answer: tx.answer,
            commands: tx.commands,
            timestamp: tx.timestamp,
        })
        .collect();

    Ok(BranchState {
        branch: branch_info,
        schema,
        entities: entity_states,
        investigations,
        recent_transactions,
    })
}

/// Caps the first ancestry element's branch_point_tx to min(current, bound).
fn cap_ancestry(ancestry: &[(Uuid, Uuid)], bound: Uuid) -> Vec<(Uuid, Uuid)> {
    let mut capped = ancestry.to_vec();
    if let Some(first) = capped.first_mut() {
        if *bound.as_bytes() < *first.1.as_bytes() {
            first.1 = bound;
        }
    }
    capped
}

fn resolve_branch(slug: &str, store: &AssertionStore) -> Result<BranchInfo, BranchStateError> {
    if slug == "main" || slug.is_empty() {
        return Ok(BranchInfo {
            id: MAIN_BRANCH,
            slug: "main".to_string(),
            reasoning: serde_json::Value::Null,
        });
    }

    let record = store
        .branches()
        .get_by_slug(slug)?
        .ok_or_else(|| BranchStateError::BranchNotFound(slug.to_string()))?;

    Ok(BranchInfo {
        id: record.id,
        slug: record.slug,
        reasoning: record.reasoning,
    })
}

fn resolve_property_full_state(
    entity_id: Uuid,
    prop: &EntityProperty,
    bound: Uuid,
    store: &AssertionStore,
) -> Result<PropertyFullState, BranchStateError> {
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

    let latest_fact = fact_col.latest_for_entity_at(prop.id, entity_id, bound)?;

    let fact_snapshot = match &latest_fact {
        Some(cell) => {
            let entry = store.facts().get_entry(
                cell.branch_id,
                cell.tx_id,
                cell.log_entry_id,
                cell.entity_id,
            )?;
            let reasoning = entry
                .map(|e| e.reasoning)
                .unwrap_or(serde_json::Value::Null);
            Some(FactSnapshot {
                value: cell.value.clone(),
                reasoning,
                tx_id: cell.tx_id,
            })
        }
        None => None,
    };

    let after_id = latest_fact
        .as_ref()
        .map(|c| c.log_entry_id)
        .unwrap_or(Uuid::nil());
    let hyp_cells = hyp_col.list_after_at(prop.id, entity_id, after_id, bound)?;

    let hypotheses = hyp_cells
        .into_iter()
        .map(|cell| resolve_hypothesis(cell, store))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PropertyFullState {
        slug: prop.slug.clone(),
        value_type: prop.value_type,
        fact: fact_snapshot,
        hypotheses,
    })
}

fn resolve_hypothesis(
    cell: ColumnCell,
    store: &AssertionStore,
) -> Result<HypothesisSnapshot, BranchStateError> {
    let entry = store.hypotheses().get_entry(
        cell.branch_id,
        cell.tx_id,
        cell.log_entry_id,
        cell.entity_id,
    )?;
    let reasoning = entry
        .map(|e| e.reasoning)
        .unwrap_or(serde_json::Value::Null);
    Ok(HypothesisSnapshot {
        value: cell.value,
        reasoning,
        tx_id: cell.tx_id,
    })
}
