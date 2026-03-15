//! ExecuteTransaction — commits a validated transaction to a branch.

use crate::command::Command;
use crate::command::input::transaction::TransactionInput;
use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::TxMeta;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;

/// Executes a validated domain transaction against a branch.
pub struct ExecuteTransaction;

impl Command for ExecuteTransaction {
    type Input = TransactionInput;
    type Output = Transaction<TxMeta>;

    fn execute(&self, _branch: &BranchContext, _input: Self::Input) -> Result<Self::Output, DbError> {
        todo!()
    }
}
