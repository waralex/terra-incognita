//! StateQuery — parameters for collecting the current branch state.

use uuid::Uuid;

/// Limits for state collection.
pub struct StateSettings {
    /// Maximum number of unique entities to include (by touched recency).
    pub touch_limit: usize,
    /// Maximum number of recent transactions to include.
    pub last_transaction_limit: usize,
}

/// Parameters for collecting branch state.
pub struct StateQuery {
    /// Point in time to query at. If None, uses the branch head.
    pub at_tx: Option<Uuid>,
    /// Collection limits.
    pub settings: StateSettings,
}

impl StateQuery {
    pub fn new(at_tx: Option<Uuid>, settings: StateSettings) -> Self {
        Self { at_tx, settings }
    }
}
