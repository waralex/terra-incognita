//! Entity — domain object with inline property values.
//!
//! `Entity<()>` for write input, `Entity<TxMeta>` for read output.

use serde_json::Value;

use crate::io::Slug;

/// A single property value on an entity.
///
/// Each property value may come from a different transaction,
/// so it carries its own context `M`.
#[derive(Debug, Clone)]
pub struct PropertyValue<M = ()> {
    pub property: Slug,
    pub value: Value,
    pub context: M,
}

/// An entity as seen by the caller — with its property values inline.
///
/// `M = ()` for write input, `M = TxMeta` for read output.
#[derive(Debug, Clone)]
pub struct Entity<M = ()> {
    pub slug: Slug,
    pub description: Option<Value>,
    pub properties: Vec<PropertyValue<M>>,
    pub context: M,
}

impl Entity<()> {
    /// Create a new entity input (before persisting).
    pub fn new(slug: Slug, description: Option<Value>, properties: Vec<PropertyValue>) -> Self {
        Self {
            slug,
            description,
            properties,
            context: (),
        }
    }
}
