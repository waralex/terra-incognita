//! EntityGetQuery — parameters for reading a single entity snapshot by slug.

use uuid::Uuid;

use crate::io::slug::Slug;

/// Parameters for reading a single entity snapshot.
///
/// Returns the entity as it stands now, or as of `at_tx` when given.
pub struct EntityGetQuery {
    /// Entity slug to read.
    pub entity: Slug,
    /// Optional point in time (upper bound). Defaults to the latest state.
    pub at_tx: Option<Uuid>,
    /// Exact property path and its dot-delimited descendants; None selects all.
    pub property_prefix: Option<Slug>,
    /// Relative path depth; None reads all descendants.
    pub property_depth: Option<usize>,
}

impl EntityGetQuery {
    /// Create a query for the latest snapshot of `entity`.
    pub fn new(entity: Slug) -> Self {
        Self {
            entity,
            at_tx: None,
            property_prefix: None,
            property_depth: None,
        }
    }

    pub fn with_property_depth(mut self, depth: usize) -> Self {
        self.property_depth = Some(depth);
        self
    }

    pub fn with_property_prefix(mut self, prefix: Slug) -> Self {
        self.property_prefix = Some(prefix);
        self
    }

    /// Read the snapshot as of `at_tx` instead of the latest state.
    pub fn with_at_tx(mut self, at_tx: Uuid) -> Self {
        self.at_tx = Some(at_tx);
        self
    }
}
