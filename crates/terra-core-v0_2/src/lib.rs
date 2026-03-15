//! terra-core v0.2 — simplified core with dynamic transaction metadata
//! and unified assertion model.
//!
//! See CLAUDE.md section "v0.2 Rewrite" for design rationale.

pub mod command;
pub mod config;
pub mod domain;
pub mod embed;
pub mod io;
pub mod store;
pub mod terra;

pub use terra::{Executable, Terra};
