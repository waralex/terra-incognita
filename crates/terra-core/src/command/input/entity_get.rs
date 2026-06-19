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
}

impl EntityGetQuery {
    /// Create a query for the latest snapshot of `entity`.
    pub fn new(entity: Slug) -> Self {
        Self {
            entity,
            at_tx: None,
        }
    }

    /// Read the snapshot as of `at_tx` instead of the latest state.
    pub fn with_at_tx(mut self, at_tx: Uuid) -> Self {
        self.at_tx = Some(at_tx);
        self
    }
}
