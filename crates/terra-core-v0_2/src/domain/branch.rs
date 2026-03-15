//! Branch — domain object representing a branch for read/write.
//!
//! `Branch<()>` for write input, `Branch<TxMeta>` for read output.

use serde_json::{Map, Value};

use crate::io::Slug;

/// A branch as seen by the caller.
///
/// `M = ()` before creation (caller input).
/// `M = TxMeta` after creation (with creation tx provenance).
#[derive(Debug, Clone)]
pub struct Branch<M = ()> {
    pub slug: Slug,
    pub parent: Slug,
    pub meta: Map<String, Value>,
    pub context: M,
}

impl Branch<()> {
    /// Create a new branch input (before persisting).
    pub fn new(slug: Slug, parent: Slug, meta: Map<String, Value>) -> Self {
        Self {
            slug,
            parent,
            meta,
            context: (),
        }
    }
}
