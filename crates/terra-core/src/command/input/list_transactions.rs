//! ListTransactionsQuery — parameters for listing recent transactions.

use uuid::Uuid;

/// Parameters for listing recent transactions.
pub struct ListTransactionsQuery {
    /// Point in time to query at. If None, uses the branch head.
    pub at_tx: Option<Uuid>,
    /// Maximum number of recent transactions to return.
    pub limit: usize,
}

impl ListTransactionsQuery {
    pub fn new(at_tx: Option<Uuid>, limit: usize) -> Self {
        Self { at_tx, limit }
    }
}
