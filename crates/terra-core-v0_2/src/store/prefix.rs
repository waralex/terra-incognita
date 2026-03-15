//! Shared prefix types for scanning branch-scoped entries.
//!
//! Prefix types are distinct from entry keys — a prefix is a scan boundary,
//! not a record identifier. Keeping them separate prevents silent breakage
//! if a key layout changes (e.g., branch becomes versioned).

use crate::io::DbItem;
use crate::io::key_prefix::prefix_key;
use crate::io::valid_prefix::{ValidPrefix, impl_prefix};
use crate::store::entry::entity::EntityEntry;
use crate::store::versioned_key::VersionedKey;

// Prefix for scanning all records on a given branch.
prefix_key! {
    pub struct BranchPrefix {
        branch: Slug,
    }
}

// BranchPrefix is valid for any entry whose key is VersionedKey
// (starts with branch by definition).
impl<T: DbItem> ValidPrefix<T> for BranchPrefix where T::Key: VersionedKey {}

// Prefix for scanning all versions of a specific entity on a branch.
prefix_key! {
    pub struct BranchEntityPrefix {
        branch: Slug,
        entity: Slug,
    }
}

impl_prefix!(BranchEntityPrefix => EntityEntry);
