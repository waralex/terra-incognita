//! GetTransactionQuery — parameters for retrieving a single transaction's full detail.

use uuid::Uuid;

/// Parameters for getting a full transaction detail.
///
/// If `tx_id` is None, returns the latest transaction on the current branch.
pub struct GetTransactionQuery {
    pub tx_id: Option<Uuid>,
}

impl GetTransactionQuery {
    pub fn new(tx_id: Option<Uuid>) -> Self {
        Self { tx_id }
    }
}
