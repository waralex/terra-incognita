//! Shared prefix types for scanning branch-scoped entries.
//!
//! `BranchPrefix` scans all records on a branch.
//! Per-entry prefixes (e.g. `EntityKeyPrefix`) are auto-generated
//! by `versioned_key!` macro.

use crate::io::key_prefix::prefix_key;
use crate::io::valid_prefix::ValidPrefix;
use crate::store::versioned_key::VersionedKey;

// Prefix for scanning all records on a given branch.
prefix_key! {
    pub struct BranchPrefix {
        branch: Slug,
    }
}

// BranchPrefix is valid for any key that is VersionedKey
// (starts with branch by definition).
impl<K: VersionedKey> ValidPrefix<K> for BranchPrefix {}
