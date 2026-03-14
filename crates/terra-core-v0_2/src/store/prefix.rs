//! Shared prefix types for scanning branch-scoped entries.
//!
//! Prefix types are distinct from entry keys — a prefix is a scan boundary,
//! not a record identifier. Keeping them separate prevents silent breakage
//! if a key layout changes (e.g., branch becomes versioned).

use crate::io::DbItem;
use crate::io::storage_key::storage_key;
use crate::io::valid_prefix::ValidPrefix;
use crate::store::versioned_key::VersionedKey;

// Prefix for scanning all records on a given branch.
storage_key! {
    pub struct BranchPrefix {
        branch_id: Uuid,
    }
}

// BranchPrefix is valid for any entry whose key is VersionedKey
// (starts with branch_id by definition).
impl<T: DbItem> ValidPrefix<T> for BranchPrefix where T::Key: VersionedKey {}
