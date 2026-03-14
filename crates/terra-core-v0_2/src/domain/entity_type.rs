//! Resolved entity type with its attached properties.

use crate::domain::property::Property;
use crate::io::Slug;

/// Entity type as seen by the caller — resolved from schema entries
/// across the ancestry chain, latest version, hidden filtered.
///
/// `M = ()` for write input, `M = TxMeta` for read output.
#[derive(Debug, Clone)]
pub struct EntityType<M = ()> {
    pub slug: Slug,
    pub description: Option<serde_json::Value>,
    pub properties: Vec<Property<M>>,
    pub meta: M,
}
