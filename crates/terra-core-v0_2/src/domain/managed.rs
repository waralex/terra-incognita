//! Managed — domain object for config-defined managed types (tasks, etc.).
//!
//! `Managed<()>` for write input, `Managed<TxMeta>` for read output.

use serde_json::{Map, Value};

use crate::io::Slug;

/// A managed type instance as seen by the caller.
///
/// `M = ()` for write input, `M = TxMeta` for read output.
#[derive(Debug, Clone)]
pub struct Managed<M = ()> {
    pub type_name: Slug,
    pub slug: Slug,
    pub state: Option<String>,
    pub fields: Map<String, Value>,
    pub context: M,
}

impl Managed<()> {
    /// Create a new managed item input (before persisting).
    pub fn new(
        type_name: Slug,
        slug: Slug,
        state: Option<String>,
        fields: Map<String, Value>,
    ) -> Self {
        Self {
            type_name,
            slug,
            state,
            fields,
            context: (),
        }
    }
}
