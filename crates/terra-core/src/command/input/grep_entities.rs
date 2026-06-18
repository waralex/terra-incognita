//! GrepEntitiesQuery — parameters for regex search over entities.

use uuid::Uuid;

/// Which fields a grep pattern is matched against.
///
/// Any enabled field that matches includes the whole entity. With no field
/// enabled the query matches nothing; the default enables `slug` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrepScope {
    /// Match against the entity slug.
    pub slug: bool,
    /// Match against property slugs.
    pub property: bool,
    /// Match against property values (serialized).
    pub value: bool,
    /// Match against per-property reasoning.
    pub reasoning: bool,
}

impl Default for GrepScope {
    fn default() -> Self {
        Self {
            slug: true,
            property: false,
            value: false,
            reasoning: false,
        }
    }
}

impl GrepScope {
    /// Whether any property field (property/value/reasoning) is enabled, i.e.
    /// matching requires the full entity snapshot rather than just its head.
    pub fn needs_properties(&self) -> bool {
        self.property || self.value || self.reasoning
    }
}

/// Parameters for finding entities whose selected fields match a regex.
pub struct GrepEntitiesQuery {
    /// Regex pattern, compiled at execution time.
    pub pattern: String,
    /// Fields the pattern is matched against.
    pub scope: GrepScope,
    /// Whether to include each entity's properties in the result. When false,
    /// only slug, description, and provenance are returned.
    pub include_properties: bool,
    /// Point in time to query at. If None, uses the branch head.
    pub at_tx: Option<Uuid>,
    /// Maximum number of entities to return.
    pub limit: usize,
}

impl GrepEntitiesQuery {
    /// Create a query with the default scope (slug only) and full properties.
    pub fn new(pattern: String, limit: usize) -> Self {
        Self {
            pattern,
            scope: GrepScope::default(),
            include_properties: true,
            at_tx: None,
            limit,
        }
    }

    /// Set the fields the pattern is matched against.
    pub fn scope(mut self, scope: GrepScope) -> Self {
        self.scope = scope;
        self
    }

    /// Set whether properties are included in the result.
    pub fn include_properties(mut self, include: bool) -> Self {
        self.include_properties = include;
        self
    }

    /// Set the point-in-time bound.
    pub fn at_tx(mut self, tx: Uuid) -> Self {
        self.at_tx = Some(tx);
        self
    }
}
