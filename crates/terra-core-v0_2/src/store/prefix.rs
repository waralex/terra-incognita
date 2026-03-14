//! Shared prefix types for scanning branch-scoped entries.
//!
//! Prefix types are distinct from entry keys — a prefix is a scan boundary,
//! not a record identifier. Keeping them separate prevents silent breakage
//! if a key layout changes (e.g., branch becomes versioned).

use crate::io::storage_key::storage_key;
use crate::io::valid_prefix::impl_prefix;

use crate::store::assertion_entry::AssertionEntry;
use crate::store::entity_entry::EntityEntry;
use crate::store::managed_entry::ManagedEntry;
use crate::store::schema_prop_entry::SchemaPropEntry;
use crate::store::schema_type_entry::SchemaTypeEntry;
use crate::store::slug_entry::SlugEntry;
use crate::store::transaction_entry::TransactionEntry;
use crate::store::visibility_entry::VisibilityEntry;

// Prefix for scanning all records on a given branch.
storage_key! {
    pub struct BranchPrefix(16) {
        branch_id: Uuid,
    }
}

impl_prefix!(BranchPrefix =>
    AssertionEntry,
    EntityEntry,
    ManagedEntry,
    SchemaPropEntry,
    SchemaTypeEntry,
    SlugEntry,
    TransactionEntry,
    VisibilityEntry,
);
