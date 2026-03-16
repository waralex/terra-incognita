//! Command layer — public API for terra-core.
//!
//! Each command is a struct implementing `Command` trait:
//! defines input type, output type, and execute method.
//! Inputs live in `input/`, executors in `executor/`.

use crate::io::DbError;
use crate::store::branch_context::BranchContext;

pub mod command_state;
pub mod executor;
pub mod input;

pub use command_state::CommandState;

/// A command that can be executed against a branch.
///
/// Commands accumulate writes into the shared `CommandState`.
/// The caller is responsible for calling `state.commit()` after execution.
pub trait Command {
    /// Input arguments for the command.
    type Input;
    /// Output returned on success.
    type Output;

    /// Execute the command on the given branch, accumulating writes into state.
    fn execute(
        &self,
        branch: &BranchContext,
        state: &mut CommandState,
        input: Self::Input,
    ) -> Result<Self::Output, DbError>;
}
