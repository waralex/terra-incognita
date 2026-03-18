//! GetTransactionQuery — parameters for retrieving a single transaction's full detail.
//!
//! By default, `transaction.get` is cross-branch: any tx_id can be retrieved
//! regardless of the current branch context (analogous to `git show <sha>`).
//! Set `only_current_branch` to restrict lookup to the current branch's scope.

use uuid::Uuid;

/// Parameters for getting a full transaction detail.
///
/// If `tx_id` is None, returns the latest transaction on the current branch.
/// If `only_current_branch` is true, returns an error when the transaction
/// belongs to a different branch.
pub struct GetTransactionQuery {
    pub tx_id: Option<Uuid>,
    pub only_current_branch: bool,
}

impl GetTransactionQuery {
    pub fn new(tx_id: Option<Uuid>) -> Self {
        Self { tx_id, only_current_branch: false }
    }

    pub fn only_current_branch(mut self) -> Self {
        self.only_current_branch = true;
        self
    }
}
