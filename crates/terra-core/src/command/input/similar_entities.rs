//! SimilarEntitiesQuery — parameters for multi-vector semantic entity search.

use uuid::Uuid;

/// Parameters for finding entities similar to one or more query values.
///
/// Each query value is converted to YAML text, embedded, and scored against
/// entity embeddings. Per entity, the maximum similarity across all queries
/// is used as the final score.
pub struct SimilarEntitiesQuery {
    /// Query values to embed and search against. Each is serialized to YAML text.
    pub queries: Vec<serde_json::Value>,
    /// Point in time to query at. If None, uses the branch head.
    pub at_tx: Option<Uuid>,
    /// Maximum number of results to return.
    pub limit: usize,
    /// Minimum cosine similarity threshold.
    pub min_similarity: f32,
}

impl SimilarEntitiesQuery {
    pub fn new(queries: Vec<serde_json::Value>, limit: usize, min_similarity: f32) -> Self {
        Self {
            queries,
            at_tx: None,
            limit,
            min_similarity,
        }
    }

    pub fn at_tx(mut self, tx: Uuid) -> Self {
        self.at_tx = Some(tx);
        self
    }
}
