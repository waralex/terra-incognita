//! TouchedEntitiesQuery — parameters for listing recently touched entities.

use uuid::Uuid;

/// Parameters for listing recently touched entities.
pub struct TouchedEntitiesQuery {
    /// Point in time to query at. If None, uses the branch head.
    pub at_tx: Option<Uuid>,
    /// Maximum number of unique entities to return (by touched recency).
    pub limit: usize,
}

impl TouchedEntitiesQuery {
    pub fn new(at_tx: Option<Uuid>, limit: usize) -> Self {
        Self { at_tx, limit }
    }
}
