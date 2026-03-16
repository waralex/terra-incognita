//! terra-core — epistemic store with dynamic transaction metadata
//! and unified assertion model.

pub mod command;
pub mod config;
pub mod domain;
pub mod embed;
pub mod io;
pub mod store;
pub mod terra;

pub use terra::{Executable, Terra};
