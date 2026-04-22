//! Semantic similarity queries — embedding-based entity search with ancestry walk.

use std::collections::HashMap;

use uuid::Uuid;

use crate::embed::cosine_similarity;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::embedding::{EmbeddingEntry, EmbeddingKey};

/// Find entities with embeddings similar to the query vector.
///
/// Walks the current branch and ancestry chain, collecting the latest
/// embedding per entity, then ranks by cosine similarity.
pub fn similar_entities(
    branch: &BranchContext,
    query_embedding: &[f32],
    limit: usize,
    min_similarity: f32,
    at_tx: Option<Uuid>,
) -> Result<Vec<SimilarityMatch>, DbError> {
    similar_entities_multi(
        branch,
        &[query_embedding.to_vec()],
        limit,
        min_similarity,
        at_tx,
    )
}

/// Result of a multi-vector similarity search: entity slug, best score, and
/// the index of the query embedding that produced the best match.
pub struct SimilarityMatch {
    pub slug: Slug,
    pub similarity: f32,
    pub matched_query: usize,
}

/// Multi-vector semantic search: accepts multiple query embeddings, performs a single
/// scan over entity embeddings, and scores each entity by the maximum cosine similarity
/// across all query vectors. Returns the index of the best-matching query per entity.
pub fn similar_entities_multi(
    branch: &BranchContext,
    query_embeddings: &[Vec<f32>],
    limit: usize,
    min_similarity: f32,
    at_tx: Option<Uuid>,
) -> Result<Vec<SimilarityMatch>, DbError> {
    if query_embeddings.is_empty() {
        return Ok(Vec::new());
    }

    let mut latest: HashMap<Slug, Vec<f32>> = HashMap::new();

    let scopes: Vec<_> = match at_tx {
        Some(tx) => branch.scopes_at(tx).collect(),
        None => branch.scopes().collect(),
    };
    for scope in &scopes {
        collect_embeddings(branch, &scope.branch, scope.upper_tx, &mut latest)?;
    }

    let mut scored: Vec<SimilarityMatch> = latest
        .into_iter()
        .filter_map(|(slug, emb)| {
            let (best_idx, best_sim) = query_embeddings
                .iter()
                .enumerate()
                .map(|(i, q)| (i, cosine_similarity(q, &emb)))
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;
            if best_sim >= min_similarity {
                Some(SimilarityMatch {
                    slug,
                    similarity: best_sim,
                    matched_query: best_idx,
                })
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);
    Ok(scored)
}

/// Collect latest embedding per entity on a single branch.
fn collect_embeddings(
    branch: &BranchContext,
    on_branch: &Slug,
    at_tx: Option<Uuid>,
    result: &mut HashMap<Slug, Vec<f32>>,
) -> Result<(), DbError> {
    let entity_bound = EmbeddingKey::bound().with_prefix(|k| {
        k.branch = on_branch.clone();
    });

    let mut iter = branch.storage().scan::<EmbeddingEntry>(&entity_bound)?;

    loop {
        let entry = match iter.next() {
            Some(Ok(e)) => e,
            Some(Err(e)) => return Err(e),
            None => break,
        };

        let entity = entry.key.entity.clone();

        if !result.contains_key(&entity) {
            let mut bound = EmbeddingKey::bound().with_prefix(|k| {
                k.branch = on_branch.clone();
                k.entity = entity.clone();
            });
            if let Some(tx) = at_tx {
                bound = bound.with_upper(|k| k.tx_id = tx);
            }

            if let Some(latest) = branch.storage().get_latest::<EmbeddingEntry>(&bound)? {
                if latest.value.is_active() {
                    result.insert(entity.clone(), latest.value.embedding);
                }
            }
        }

        let skip = EmbeddingKey::bound().with_prefix(|k| {
            k.branch = on_branch.clone();
            k.entity = entity.clone();
            k.tx_id = Uuid::max();
        });
        iter.seek(&skip);
    }

    Ok(())
}
