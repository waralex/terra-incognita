//! Resolved property of an entity type.

use crate::io::Slug;

/// Property as seen by the caller — latest version from ancestry chain.
///
/// `M = ()` for write input, `M = TxMeta` for read output.
#[derive(Debug, Clone)]
pub struct Property<M = ()> {
    pub slug: Slug,
    pub description: Option<serde_json::Value>,
    pub meta: M,
}
