//! Command layer — public write API for terra-core.
//!
//! Accepts validated domain objects, converts them to store entries,
//! and commits via WriteBatch. Callers create domain objects through
//! DomainFactory, handle validation errors, then pass valid objects here.

pub mod executor;
pub mod input;
