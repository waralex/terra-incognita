use std::sync::{Arc, Mutex};
use terra_core::schema::SchemaRegistry;

pub type AppState = Arc<Mutex<SchemaRegistry>>;
