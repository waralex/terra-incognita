//! Command layer — public API for terra-core.
//!
//! Each command is a struct implementing `Command` trait:
//! defines input type, output type, and execute method.
//! Inputs live in `input/`, executors in `executor/`.

use crate::io::DbError;
use crate::store::branch_context::BranchContext;

pub mod executor;
pub mod input;

/// A command that can be executed against a branch.
pub trait Command {
    /// Input arguments for the command.
    type Input;
    /// Output returned on success.
    type Output;

    /// Execute the command on the given branch.
    fn execute(&self, branch: &BranchContext, input: Self::Input) -> Result<Self::Output, DbError>;
}
