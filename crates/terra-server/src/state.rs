use std::sync::{Arc, Mutex};
use terra_core::assertion::AssertionStore;
use terra_core::schema::SchemaRegistry;

/// Shared mutable state holding both stores.
pub struct Inner {
    pub registry: SchemaRegistry,
    pub assertions: AssertionStore,
}

/// Thread-safe shared application state.
pub type AppState = Arc<Mutex<Inner>>;
