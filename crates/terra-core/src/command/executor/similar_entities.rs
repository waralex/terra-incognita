//! FindSimilarEntities — multi-vector semantic entity search with full entity loading.

use crate::command::input::similar_entities::SimilarEntitiesQuery;
use crate::command::Command;
use crate::command::CommandState;
use crate::domain::entity::SimilarEntity;
use crate::domain::tx_meta::TxMeta;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::query::{entity_snapshot, similarity};

/// Accepts multiple query values, embeds each, and returns entities ranked
/// by the maximum cosine similarity across all query vectors.
/// Each result includes the full entity snapshot and the index of the
/// best-matching query.
pub struct FindSimilarEntities;

impl Command for FindSimilarEntities {
    type Input = SimilarEntitiesQuery;
    type Output = Vec<SimilarEntity<TxMeta>>;

    fn execute(
        &self,
        branch: &BranchContext,
        state: &mut CommandState,
        input: Self::Input,
    ) -> Result<Self::Output, DbError> {
        let embedder = state.embedder();
        let embeddings: Vec<Vec<f32>> = input
            .queries
            .iter()
            .map(|v| {
                let text = serde_yaml::to_string(v).map_err(|e| DbError::Storage(e.to_string()))?;
                embedder.embed(&text)
            })
            .collect::<Result<_, _>>()?;

        let matches = similarity::similar_entities_multi(
            branch,
            &embeddings,
            input.limit,
            input.min_similarity,
            input.at_tx,
        )?;

        let at_tx = input.at_tx;
        let mut results = Vec::with_capacity(matches.len());

        for m in matches {
            if let Some(entity) = entity_snapshot::entity_snapshot(branch, &m.slug, at_tx)? {
                results.push(SimilarEntity {
                    entity,
                    similarity: m.similarity,
                    matched_query: m.matched_query,
                });
            }
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use indoc::indoc;
    use serde_json::{Map, Value};

    use super::*;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::transaction::TransactionInput;
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::{Entity, PropertyValue as PV};
    use crate::domain::validator::DomainValidator;
    use crate::embed::Embedder;
    use crate::store::storage::Storage;

    struct TestEmbedder {
        calls: Mutex<Vec<String>>,
    }

    impl TestEmbedder {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl Embedder for TestEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>, DbError> {
            self.calls.lock().unwrap().push(text.to_string());
            let len = text.len() as f32;
            Ok(vec![len, len * 0.5, len * 0.1, 1.0])
        }

        fn dimensions(&self) -> usize {
            4
        }
    }

    fn test_config() -> Arc<ProjectConfig> {
        Arc::new(
            ProjectConfig::builder()
                .data_dir("./data".into())
                .schema_path("./schema.yaml".into())
                .build(),
        )
    }

    fn test_schema() -> Arc<DataSchema> {
        Arc::new(
            DataSchema::from_yaml(indoc! {"
            transaction_meta:
              reasoning:
                type: text
                required: true
            entity_change_meta:
              reasoning:
                type: text
                required: true
            branch_meta:
              reasoning:
                type: text
                required: true
            managed_types:
              task:
                fields:
                  goal: { type: json, required: true }
                lifecycle:
                  initial: open
                  visible: [open]
        "})
            .unwrap(),
        )
    }

    fn meta(r: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(r.into()));
        m
    }

    fn exec_tx_with_embedder(
        branch: &BranchContext,
        embedder: Arc<dyn Embedder>,
        input: TransactionInput,
    ) {
        let cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::with_embedder(branch.storage(), embedder);
        cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
    }

    fn find(
        branch: &BranchContext,
        embedder: Arc<dyn Embedder>,
        input: SimilarEntitiesQuery,
    ) -> Vec<SimilarEntity<TxMeta>> {
        let cmd = FindSimilarEntities;
        let mut state = CommandState::with_embedder(branch.storage(), embedder);
        cmd.execute(branch, &mut state, input).unwrap()
    }

    #[test]
    fn single_query_finds_similar() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let embedder: Arc<dyn Embedder> = Arc::new(TestEmbedder::new());

        exec_tx_with_embedder(
            &branch,
            embedder.clone(),
            TransactionInput::new(meta("init")).create_entity(Entity::new(
                "auth-service".parse().unwrap(),
                Some(serde_json::json!("auth service")),
                vec![PV {
                    property: "role".parse().unwrap(),
                    value: serde_json::json!("authentication"),
                    context: (),
                }],
                meta("setup"),
            )),
        );

        let results = find(
            &branch,
            embedder,
            SimilarEntitiesQuery::new(vec![serde_json::json!("auth middleware")], 10, 0.0),
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity.slug.as_str(), "auth-service");
        assert!(results[0].similarity > 0.0);
        assert_eq!(results[0].matched_query, 0);
        assert_eq!(results[0].entity.properties.len(), 1);
        assert_eq!(
            results[0].entity.description,
            Some(serde_json::json!("auth service"))
        );
    }

    #[test]
    fn multi_query_union() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let embedder: Arc<dyn Embedder> = Arc::new(TestEmbedder::new());

        exec_tx_with_embedder(
            &branch,
            embedder.clone(),
            TransactionInput::new(meta("init"))
                .create_entity(Entity::new(
                    "auth-service".parse().unwrap(),
                    Some(serde_json::json!("auth")),
                    vec![],
                    meta("setup"),
                ))
                .create_entity(Entity::new(
                    "payment-service".parse().unwrap(),
                    Some(serde_json::json!("payments")),
                    vec![],
                    meta("setup"),
                )),
        );

        let results = find(
            &branch,
            embedder,
            SimilarEntitiesQuery::new(
                vec![
                    serde_json::json!("authentication"),
                    serde_json::json!("billing"),
                ],
                10,
                0.0,
            ),
        );

        assert_eq!(results.len(), 2);
        let slugs: Vec<&str> = results.iter().map(|r| r.entity.slug.as_str()).collect();
        assert!(slugs.contains(&"auth-service"));
        assert!(slugs.contains(&"payment-service"));
    }

    #[test]
    fn min_similarity_filters() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let embedder: Arc<dyn Embedder> = Arc::new(TestEmbedder::new());

        exec_tx_with_embedder(
            &branch,
            embedder.clone(),
            TransactionInput::new(meta("init")).create_entity(Entity::new(
                "low-match".parse().unwrap(),
                Some(serde_json::json!("something")),
                vec![],
                meta("setup"),
            )),
        );

        let results = find(
            &branch,
            embedder,
            SimilarEntitiesQuery::new(vec![serde_json::json!("query")], 10, 0.9999),
        );

        assert!(results.is_empty() || results[0].similarity >= 0.9999);
    }

    #[test]
    fn empty_queries_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let embedder: Arc<dyn Embedder> = Arc::new(TestEmbedder::new());

        let results = find(
            &branch,
            embedder,
            SimilarEntitiesQuery::new(vec![], 10, 0.0),
        );

        assert!(results.is_empty());
    }
}
