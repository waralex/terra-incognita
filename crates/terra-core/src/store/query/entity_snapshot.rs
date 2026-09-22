//! Entity snapshot — load a single entity with properties at a point in time.

use uuid::Uuid;

use crate::config::AssertionStatusesDef;
use crate::domain::entity::{Entity, PropertyValue};
use crate::domain::tx_meta::{time_from_uuid, TxMeta};
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::entity::{EntityEntry, EntityKey};
use crate::store::query::properties;

/// The record-level part of an entity at a point in time, without properties.
pub struct EntityHead {
    /// tx_id of the latest entity record (provenance + recency ordering).
    pub tx_id: Uuid,
    /// Branch where that record lives.
    pub branch: Slug,
    /// Entity description, if any.
    pub description: Option<serde_json::Value>,
}

/// Load just the entity record head (no properties) at an optional point in time.
///
/// Returns `None` if the entity is deleted or has no record. This is the cheap
/// path for callers that only need the slug and provenance — it skips the
/// per-property assertion scan that [`entity_snapshot`] performs.
pub fn entity_head(
    branch: &BranchContext,
    slug: &Slug,
    at_tx: Option<Uuid>,
) -> Result<Option<EntityHead>, DbError> {
    let mut bound = EntityKey::bound().with_prefix(|k| k.entity = slug.clone());
    if let Some(tx) = at_tx {
        bound = bound.with_upper(|k| k.tx_id = tx);
    }

    let entry = match branch.get_latest::<EntityEntry>(&bound)? {
        Some(e) if !e.value.is_deleted() => e,
        _ => return Ok(None),
    };

    Ok(Some(EntityHead {
        tx_id: entry.key.tx_id,
        branch: entry.key.branch,
        description: entry.value.description,
    }))
}

/// Load a single entity snapshot (record + properties) at an optional point in time.
///
/// When `statuses` is `Some`, properties are layered: the latest terminal assertion
/// is the baseline and later non-terminal assertions are stacked on top, each
/// carrying its status in the property context. When `None`, the snapshot is the
/// plain latest assertion per property.
///
/// Returns `None` if the entity is deleted or has no record.
pub fn entity_snapshot(
    branch: &BranchContext,
    slug: &Slug,
    at_tx: Option<Uuid>,
    statuses: Option<&AssertionStatusesDef>,
) -> Result<Option<Entity<TxMeta>>, DbError> {
    entity_snapshot_selected(branch, slug, at_tx, statuses, None, None)
}

pub(crate) fn entity_snapshot_selected(
    branch: &BranchContext,
    slug: &Slug,
    at_tx: Option<Uuid>,
    statuses: Option<&AssertionStatusesDef>,
    prefix: Option<&Slug>,
    depth: Option<usize>,
) -> Result<Option<Entity<TxMeta>>, DbError> {
    let Some(head) = entity_head(branch, slug, at_tx)? else {
        return Ok(None);
    };

    let (assertion_entries, refs) = if prefix.is_some() || depth.is_some() {
        properties::selected_properties(branch, slug, at_tx, statuses, prefix, depth)?
    } else {
        (
            match statuses {
                Some(s) => properties::layered_properties(branch, slug, at_tx, s)?,
                None => properties::properties(branch, slug, at_tx)?,
            },
            vec![],
        )
    };
    let properties: Vec<PropertyValue<TxMeta>> = assertion_entries
        .into_iter()
        .map(|a| {
            let status = statuses.map(|s| s.resolve(a.value.status.as_deref()).to_string());
            PropertyValue {
                supersedes_tx: a.value.supersedes_tx,
                property: a.key.prop,
                value: a.value.value.clone(),
                context: TxMeta {
                    tx_id: a.key.tx_id,
                    branch: a.key.branch,
                    reasoning: Some(a.value.reasoning),
                    time: time_from_uuid(a.key.tx_id),
                    status,
                    source: a.value.source,
                },
            }
        })
        .collect();

    let mut entity = head_entity(slug, head, properties);
    entity.property_refs = refs;
    Ok(Some(entity))
}

/// Assemble a read-side entity from its head record and (possibly empty)
/// property list. The single place that decides what an entity's own
/// `context` carries, shared by the snapshot and the head-only paths.
pub fn head_entity(
    slug: &Slug,
    head: EntityHead,
    properties: Vec<PropertyValue<TxMeta>>,
) -> Entity<TxMeta> {
    Entity {
        property_refs: vec![],
        slug: slug.clone(),
        description: head.description,
        properties,
        meta: serde_json::Map::new(),
        status: None,
        context: TxMeta {
            tx_id: head.tx_id,
            branch: head.branch,
            reasoning: None,
            time: time_from_uuid(head.tx_id),
            status: None,
            source: None,
        },
    }
}
