//! Entity enumeration — distinct entity slugs on a branch and its ancestry.

use std::collections::HashSet;

use uuid::Uuid;

use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::entity::{EntityEntry, EntityKey};

/// Collect the distinct entity slugs visible on this branch (current branch plus
/// ancestry). Liveness (deletion, point-in-time) is intentionally not resolved
/// here — callers decide per slug via [`crate::store::query::entity_snapshot`]
/// or [`crate::store::query::entity_snapshot::entity_head`], which walk the
/// ancestry chain correctly.
pub fn entity_slugs(branch: &BranchContext) -> Result<Vec<Slug>, DbError> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for scope in branch.scopes() {
        collect_slugs(branch, &scope.branch, &mut seen, &mut result)?;
    }

    Ok(result)
}

/// Forward-scan one branch, seeking past every version of each entity so we read
/// roughly one entry per entity rather than the full version history.
fn collect_slugs(
    branch: &BranchContext,
    on_branch: &Slug,
    seen: &mut HashSet<Slug>,
    result: &mut Vec<Slug>,
) -> Result<(), DbError> {
    let bound = EntityKey::bound().with_prefix(|k| k.branch = on_branch.clone());
    let mut iter = branch.storage().scan::<EntityEntry>(&bound)?;

    loop {
        let entry = match iter.next() {
            Some(Ok(e)) => e,
            Some(Err(e)) => return Err(e),
            None => break,
        };

        let entity = entry.key.entity.clone();
        if seen.insert(entity.clone()) {
            result.push(entity.clone());
        }

        let skip = EntityKey::bound().with_prefix(|k| {
            k.branch = on_branch.clone();
            k.entity = entity.clone();
            k.tx_id = Uuid::max();
        });
        iter.seek(&skip);
    }

    Ok(())
}
