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
    let mut bound = EntityKey::bound().with_prefix(|k| k.entity = slug.clone());
    if let Some(tx) = at_tx {
        bound = bound.with_upper(|k| k.tx_id = tx);
    }

    let entry = branch.get_latest::<EntityEntry>(&bound)?;
    if entry.as_ref().is_some_and(|e| e.value.is_deleted()) {
        return Ok(None);
    }

    let entity_tx = entry.as_ref().map(|e| e.key.tx_id).unwrap_or(Uuid::nil());
    let entity_branch = entry
        .as_ref()
        .map(|e| e.key.branch.clone())
        .unwrap_or_else(|| branch.id().clone());
    let description = entry.and_then(|e| e.value.description);

    let assertion_entries = match statuses {
        Some(s) => properties::layered_properties(branch, slug, at_tx, s)?,
        None => properties::properties(branch, slug, at_tx)?,
    };
    let properties: Vec<PropertyValue<TxMeta>> = assertion_entries
        .into_iter()
        .map(|a| {
            let status = statuses.map(|s| s.resolve(a.value.status.as_deref()).to_string());
            PropertyValue {
                property: a.key.prop,
                value: a.value.value.clone(),
                context: TxMeta {
                    tx_id: a.key.tx_id,
                    branch: a.key.branch,
                    reasoning: Some(a.value.reasoning),
                    time: time_from_uuid(a.key.tx_id),
                    status,
                },
            }
        })
        .collect();

    Ok(Some(Entity {
        slug: slug.clone(),
        description,
        properties,
        meta: serde_json::Map::new(),
        status: None,
        context: TxMeta {
            tx_id: entity_tx,
            branch: entity_branch,
            reasoning: None,
            time: time_from_uuid(entity_tx),
            status: None,
        },
    }))
}
