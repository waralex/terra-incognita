use std::sync::{Arc, Mutex};
use terra_core::assertion::AssertionStore;
use terra_core::schema::SchemaRegistry;

pub struct Inner {
    pub registry: SchemaRegistry,
    pub assertions: AssertionStore,
}

pub type AppState = Arc<Mutex<Inner>>;
