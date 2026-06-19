//! `Terra` — library facade that owns config, schema, validator, embedder
//! and exposes a single `execute` entry point.

use std::path::Path;
use std::sync::Arc;

use crate::command::executor::checkout::{CheckoutOutput, ExecuteCheckout};
use crate::command::executor::entity_get::GetEntity;
use crate::command::executor::entity_history::ListEntityHistory;
use crate::command::executor::get_branch::GetBranch;
use crate::command::executor::get_transaction::GetTransaction;
use crate::command::executor::grep_entities::GrepEntities;
use crate::command::executor::list_managed::ListManaged;
use crate::command::executor::list_transactions::ListTransactions;
use crate::command::executor::similar_entities::FindSimilarEntities;
use crate::command::executor::touched_entities::ListTouchedEntities;
use crate::command::executor::transaction::ExecuteTransaction;
use crate::command::input::checkout::CheckoutInput;
use crate::command::input::entity_get::EntityGetQuery;
use crate::command::input::entity_history::EntityHistoryQuery;
use crate::command::input::get_branch::GetBranchQuery;
use crate::command::input::get_transaction::GetTransactionQuery;
use crate::command::input::grep_entities::GrepEntitiesQuery;
use crate::command::input::list_managed::ListManagedQuery;
use crate::command::input::list_transactions::ListTransactionsQuery;
use crate::command::input::similar_entities::SimilarEntitiesQuery;
use crate::command::input::touched_entities::TouchedEntitiesQuery;
use crate::command::input::transaction::TransactionInput;
use crate::command::{Command, CommandState};
use crate::config::{DataSchema, ProjectConfig};
use crate::domain::branch::Branch;
use crate::domain::entity::{Entity, SimilarEntity};
use crate::domain::entity_history::EntityHistoryEntry;
use crate::domain::managed::Managed;
use crate::domain::transaction::{Transaction, TransactionDetail};
use crate::domain::tx_meta::TxMeta;
use crate::domain::validator::DomainValidator;
use crate::embed::Embedder;
use crate::io::slug::Slug;
use crate::io::DbError;
use crate::store::branch_context::{main_branch_slug, BranchContext};
use crate::store::storage::Storage;

/// A command input that can be executed against a branch via `Terra`.
///
/// Each input type implements this to wire itself to the right executor.
pub trait Executable {
    /// Output returned on success.
    type Output;

    /// Execute this input on the given branch, accumulating writes into state.
    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError>;
}

/// Single entry point for all terra-core operations.
///
/// Owns storage, validator, schema, and embedder. Resolves branches by slug
/// and handles CommandState lifecycle (create → execute → commit).
pub struct Terra {
    storage: Storage,
    validator: DomainValidator,
    schema: Arc<DataSchema>,
    embedder: Arc<dyn Embedder>,
}

impl Terra {
    /// Open terra with the given config, schema, and embedder.
    pub fn open(
        path: &Path,
        config: Arc<ProjectConfig>,
        schema: Arc<DataSchema>,
        embedder: Arc<dyn Embedder>,
    ) -> Result<Self, DbError> {
        let storage = Storage::open(path, config)?;
        let validator = DomainValidator::new(schema.clone());
        Ok(Self {
            storage,
            validator,
            schema,
            embedder,
        })
    }

    /// Execute a command on the given branch.
    ///
    /// Resolves the branch slug, creates a CommandState, delegates to
    /// `Executable::execute_on`, and commits the batch atomically.
    pub fn execute<E: Executable>(&self, branch: &Slug, input: E) -> Result<E::Output, DbError> {
        let ctx = self.resolve_branch(branch)?;
        let mut state = CommandState::with_embedder(&self.storage, self.embedder.clone());
        let output = input.execute_on(self, &ctx, &mut state)?;
        state.commit()?;
        Ok(output)
    }

    /// Access the underlying storage for advanced usage.
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    fn resolve_branch(&self, slug: &Slug) -> Result<BranchContext, DbError> {
        if *slug == main_branch_slug() {
            Ok(self.storage.main_branch())
        } else {
            self.storage.branch(slug.clone())
        }
    }
}

// --- Executable impls ---

impl Executable for TransactionInput {
    type Output = Transaction<TxMeta>;

    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        ExecuteTransaction::new(terra.validator.clone()).execute(branch, state, self)
    }
}

impl Executable for CheckoutInput {
    type Output = CheckoutOutput;

    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        ExecuteCheckout::new(terra.validator.clone()).execute(branch, state, self)
    }
}

impl Executable for GetTransactionQuery {
    type Output = TransactionDetail;

    fn execute_on(
        self,
        _terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        GetTransaction.execute(branch, state, self)
    }
}

impl Executable for ListTransactionsQuery {
    type Output = Vec<Transaction<TxMeta>>;

    fn execute_on(
        self,
        _terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        ListTransactions.execute(branch, state, self)
    }
}

impl Executable for TouchedEntitiesQuery {
    type Output = Vec<Entity<TxMeta>>;

    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        ListTouchedEntities::new(terra.schema.clone()).execute(branch, state, self)
    }
}

impl Executable for GetBranchQuery {
    type Output = Branch<TxMeta>;

    fn execute_on(
        self,
        _terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        GetBranch.execute(branch, state, self)
    }
}

impl Executable for ListManagedQuery {
    type Output = Vec<Managed<TxMeta>>;

    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        ListManaged::new(terra.schema.clone()).execute(branch, state, self)
    }
}

impl Executable for EntityGetQuery {
    type Output = Entity<TxMeta>;

    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        GetEntity::new(terra.schema.clone()).execute(branch, state, self)
    }
}

impl Executable for EntityHistoryQuery {
    type Output = Vec<EntityHistoryEntry>;

    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        ListEntityHistory::new(terra.schema.clone()).execute(branch, state, self)
    }
}

impl Executable for GrepEntitiesQuery {
    type Output = Vec<Entity<TxMeta>>;

    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        GrepEntities::new(terra.schema.clone()).execute(branch, state, self)
    }
}

impl Executable for SimilarEntitiesQuery {
    type Output = Vec<SimilarEntity<TxMeta>>;

    fn execute_on(
        self,
        terra: &Terra,
        branch: &BranchContext,
        state: &mut CommandState,
    ) -> Result<Self::Output, DbError> {
        FindSimilarEntities::new(terra.schema.clone()).execute(branch, state, self)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use indoc::indoc;
    use serde_json::{Map, Value};

    use super::*;
    use crate::config::DataSchema;
    use crate::domain::entity::{Entity, PropertyValue as PV};
    use crate::embed::{Embedder, NoopEmbedder};

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

    fn open_terra(dir: &Path) -> Terra {
        Terra::open(dir, test_config(), test_schema(), Arc::new(NoopEmbedder)).unwrap()
    }

    #[test]
    fn execute_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());
        let main = main_branch_slug();

        let tx = terra
            .execute(
                &main,
                TransactionInput::new(meta("create entity")).create_entity(Entity::new(
                    "alice".parse().unwrap(),
                    Some(serde_json::json!("A person")),
                    vec![PV {
                        property: "age".parse().unwrap(),
                        value: serde_json::json!(25),
                        context: (),
                    }],
                    meta("initial"),
                )),
            )
            .unwrap();

        assert_eq!(tx.context.branch, main);

        let entities = terra
            .execute(&main, TouchedEntitiesQuery::new(None, 10))
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].slug.as_str(), "alice");
        assert_eq!(entities[0].properties[0].value, serde_json::json!(25));
    }

    #[test]
    fn execute_list_transactions() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());
        let main = main_branch_slug();

        terra
            .execute(&main, TransactionInput::new(meta("first")))
            .unwrap();
        terra
            .execute(&main, TransactionInput::new(meta("second")))
            .unwrap();

        let txs = terra
            .execute(&main, ListTransactionsQuery::new(None, 10))
            .unwrap();

        assert_eq!(txs.len(), 2);
        assert_eq!(txs[0].meta["reasoning"], "second");
        assert_eq!(txs[1].meta["reasoning"], "first");
    }

    #[test]
    fn execute_checkout() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());
        let main = main_branch_slug();

        terra
            .execute(
                &main,
                TransactionInput::new(meta("seed")).create_entity(Entity::new(
                    "alice".parse().unwrap(),
                    Some(serde_json::json!("A person")),
                    vec![],
                    Map::new(),
                )),
            )
            .unwrap();

        let checkout = terra
            .execute(
                &main,
                CheckoutInput::new(
                    "feature".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("first on branch")),
                ),
            )
            .unwrap();

        assert_eq!(checkout.branch.as_str(), "feature");

        let branch_slug: Slug = "feature".parse().unwrap();
        let branch = terra.execute(&branch_slug, GetBranchQuery::new()).unwrap();
        assert_eq!(branch.slug.as_str(), "feature");
        assert_eq!(branch.parent.as_str(), "main");
    }

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

    #[test]
    fn execute_similar_entities() {
        let dir = tempfile::tempdir().unwrap();
        let embedder: Arc<dyn Embedder> = Arc::new(TestEmbedder::new());
        let terra = Terra::open(dir.path(), test_config(), test_schema(), embedder).unwrap();
        let main = main_branch_slug();

        terra
            .execute(
                &main,
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
            )
            .unwrap();

        let results = terra
            .execute(
                &main,
                SimilarEntitiesQuery::new(vec![serde_json::json!("auth middleware")], 10, 0.0),
            )
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].entity.slug.as_str(), "auth-service");
        assert!(results[0].similarity > 0.0);
        assert_eq!(results[0].matched_query, 0);
    }

    #[test]
    fn layered_snapshot_surfaces_status() {
        let dir = tempfile::tempdir().unwrap();
        let schema = Arc::new(
            DataSchema::from_yaml(indoc! {"
                transaction_meta:
                  reasoning: { type: text, required: true }
                entity_change_meta:
                  reasoning: { type: text, required: true }
                assertion_statuses:
                  values: [fact, hypothesis]
                  terminal: fact
                  default: hypothesis
            "})
            .unwrap(),
        );
        let terra = Terra::open(dir.path(), test_config(), schema, Arc::new(NoopEmbedder)).unwrap();
        let main = main_branch_slug();

        terra
            .execute(
                &main,
                TransactionInput::new(meta("settle")).create_entity(
                    Entity::new(
                        "alice".parse().unwrap(),
                        Some(serde_json::json!("A person")),
                        vec![PV {
                            property: "city".parse().unwrap(),
                            value: serde_json::json!("Paris"),
                            context: (),
                        }],
                        meta("settled"),
                    )
                    .with_status(Some("fact".into())),
                ),
            )
            .unwrap();

        terra
            .execute(
                &main,
                TransactionInput::new(meta("guess")).update_entity(
                    Entity::new(
                        "alice".parse().unwrap(),
                        None,
                        vec![PV {
                            property: "city".parse().unwrap(),
                            value: serde_json::json!("Lyon?"),
                            context: (),
                        }],
                        meta("a guess"),
                    )
                    .with_status(Some("hypothesis".into())),
                ),
            )
            .unwrap();

        let entities = terra
            .execute(&main, TouchedEntitiesQuery::new(None, 10))
            .unwrap();
        let props = &entities[0].properties;
        assert_eq!(props.len(), 2);
        assert!(props
            .iter()
            .any(|p| p.value == serde_json::json!("Paris")
                && p.context.status.as_deref() == Some("fact")));
        assert!(props
            .iter()
            .any(|p| p.context.status.as_deref() == Some("hypothesis")));
    }

    #[test]
    fn execute_empty_branch() {
        let dir = tempfile::tempdir().unwrap();
        let terra = open_terra(dir.path());
        let main = main_branch_slug();

        let txs = terra
            .execute(&main, ListTransactionsQuery::new(None, 10))
            .unwrap();
        assert!(txs.is_empty());

        let entities = terra
            .execute(&main, TouchedEntitiesQuery::new(None, 10))
            .unwrap();
        assert!(entities.is_empty());
    }
}
