//! ExecuteTransaction — commits a validated transaction to a branch.

use crate::store::branch_context::BranchContext;

/// Executes a validated domain transaction against a branch.
pub struct ExecuteTransaction {
    branch: BranchContext,
}

impl ExecuteTransaction {
    /// Create a new command bound to a branch.
    pub fn new(branch: BranchContext) -> Self {
        Self { branch }
    }
}
