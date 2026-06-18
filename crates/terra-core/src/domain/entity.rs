//! Entity — domain object with inline property values.
//!
//! `Entity<()>` for write input, `Entity<TxMeta>` for read output.

use serde_json::{Map, Value};

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
    /// Entity change metadata — validated against `DataSchema.entity_change_meta`.
    pub meta: Map<String, Value>,
    /// Epistemic status applied to this change's assertions (per `assertion_statuses`).
    /// `None` on write → schema `default`. Always `None` on read output (a snapshot
    /// has no single status; per-property status lives in each property's context).
    pub status: Option<String>,
    pub context: M,
}

/// Entity with a similarity score — returned by semantic search.
#[derive(Debug, Clone)]
pub struct SimilarEntity<M = ()> {
    pub entity: Entity<M>,
    pub similarity: f32,
    /// Index of the query that produced the best match.
    pub matched_query: usize,
}

impl Entity<()> {
    /// Create a new entity input (before persisting).
    pub fn new(
        slug: Slug,
        description: Option<Value>,
        properties: Vec<PropertyValue>,
        meta: Map<String, Value>,
    ) -> Self {
        Self {
            slug,
            description,
            properties,
            meta,
            status: None,
            context: (),
        }
    }

    /// Set the epistemic status for this change's assertions.
    pub fn with_status(mut self, status: Option<String>) -> Self {
        self.status = status;
        self
    }
}
