use std::sync::{Arc, Mutex};
use terra_core::assertion::AssertionStore;

/// Shared mutable state holding the assertion store.
pub struct Inner {
    pub assertions: AssertionStore,
}

/// Thread-safe shared application state.
pub type AppState = Arc<Mutex<Inner>>;
