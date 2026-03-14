//! Shared prefix types for scanning branch-scoped entries.
//!
//! Prefix types are distinct from entry keys — a prefix is a scan boundary,
//! not a record identifier. Keeping them separate prevents silent breakage
//! if a key layout changes (e.g., branch becomes versioned).

use crate::io::DbItem;
use crate::io::storage_key::storage_key;
use crate::io::valid_prefix::{ValidPrefix, impl_prefix};
use crate::io::versioned_key::VersionedKey;

use crate::store::transaction_entry::TransactionEntry;

// Prefix for scanning all records on a given branch.
storage_key! {
    pub struct BranchPrefix(16) {
        branch_id: Uuid,
    }
}

// BranchPrefix is valid for any entry whose key is VersionedKey
// (starts with branch_id by definition).
impl<T: DbItem> ValidPrefix<T> for BranchPrefix where T::Key: VersionedKey {}

// TransactionEntry is not versioned but starts with branch_id.
impl_prefix!(BranchPrefix => TransactionEntry);
