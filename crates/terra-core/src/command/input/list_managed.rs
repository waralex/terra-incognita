//! ListManagedQuery — parameters for listing managed items.

use uuid::Uuid;

/// Parameters for listing managed items in visible lifecycle states.
pub struct ListManagedQuery {
    /// Point in time to query at. If None, uses the branch head.
    pub at_tx: Option<Uuid>,
}

impl ListManagedQuery {
    pub fn new(at_tx: Option<Uuid>) -> Self {
        Self { at_tx }
    }
}
