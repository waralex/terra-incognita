//! GrepEntities — regex search over entities, returning matching snapshots.

use std::sync::Arc;

use regex::Regex;
use serde_json::Value;

use crate::command::input::grep_entities::{GrepEntitiesQuery, GrepScope};
use crate::command::Command;
use crate::command::CommandState;
use crate::config::DataSchema;
use crate::domain::entity::Entity;
use crate::domain::tx_meta::{time_from_uuid, TxMeta};
use crate::domain::validator::ValidationError;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::query::entity_slugs::entity_slugs;
use crate::store::query::entity_snapshot::{entity_head, entity_snapshot};

/// Searches entities on the branch by matching a regex against the fields
/// selected in the query scope (slug, property names, values, reasoning).
/// Returns matching entity snapshots, newest first, capped at the limit.
pub struct GrepEntities {
    schema: Arc<DataSchema>,
}

impl GrepEntities {
    /// Create the executor with the project schema (for assertion-status layering).
    pub fn new(schema: Arc<DataSchema>) -> Self {
        Self { schema }
    }
}

impl Command for GrepEntities {
    type Input = GrepEntitiesQuery;
    type Output = Vec<Entity<TxMeta>>;

    fn execute(
        &self,
        branch: &BranchContext,
        _state: &mut CommandState,
        input: Self::Input,
    ) -> Result<Self::Output, DbError> {
        let re = Regex::new(&input.pattern).map_err(|e| {
            DbError::Validation(ValidationError::InvalidRegex {
                pattern: input.pattern.clone(),
                message: e.to_string(),
            })
        })?;

        let scope = input.scope;
        let at_tx = input.at_tx;
        let statuses = self.schema.assertion_statuses.as_ref();
        // The full snapshot is needed when matching against properties, or when
        // the caller wants properties in the output.
        let need_snapshot = scope.needs_properties() || input.include_properties;

        let mut results: Vec<Entity<TxMeta>> = Vec::new();
        for slug in entity_slugs(branch)? {
            let matched = if need_snapshot {
                let Some(mut entity) = entity_snapshot(branch, &slug, at_tx, statuses)? else {
                    continue;
                };
                if (scope.slug && re.is_match(slug.as_str())) || content_matches(&entity, scope, &re)
                {
                    if !input.include_properties {
                        entity.properties.clear();
                    }
                    Some(entity)
                } else {
                    None
                }
            } else {
                // Slug-only matching, slug-only output: skip the property scan.
                if !(scope.slug && re.is_match(slug.as_str())) {
                    continue;
                }
                entity_head(branch, &slug, at_tx)?.map(|head| Entity {
                    slug: slug.clone(),
                    description: head.description,
                    properties: Vec::new(),
                    meta: serde_json::Map::new(),
                    status: None,
                    context: TxMeta {
                        tx_id: head.tx_id,
                        branch: head.branch,
                        reasoning: None,
                        time: time_from_uuid(head.tx_id),
                        status: None,
                        source: None,
                    },
                })
            };

            if let Some(entity) = matched {
                results.push(entity);
            }
        }

        // Reverse insertion order: tx_id is UUID v7, so this is newest-first.
        results.sort_by(|a, b| b.context.tx_id.cmp(&a.context.tx_id));
        results.truncate(input.limit);
        Ok(results)
    }
}

/// Whether any enabled property-level field matches the pattern.
fn content_matches(entity: &Entity<TxMeta>, scope: GrepScope, re: &Regex) -> bool {
    if scope.property
        && entity
            .properties
            .iter()
            .any(|p| re.is_match(p.property.as_str()))
    {
        return true;
    }
    if scope.value
        && entity
            .properties
            .iter()
            .any(|p| value_matches(&p.value, re))
    {
        return true;
    }
    if scope.reasoning
        && entity.properties.iter().any(|p| {
            p.context
                .reasoning
                .as_deref()
                .is_some_and(|r| re.is_match(r))
        })
    {
        return true;
    }
    false
}

/// Match a property value: strings match their raw text; numbers, booleans,
/// arrays and objects match their compact JSON serialization (e.g. `42`,
/// `true`, `{"k":"v"}` — quotes and braces included, no spaces).
fn value_matches(value: &Value, re: &Regex) -> bool {
    match value.as_str() {
        Some(s) => re.is_match(s),
        None => re.is_match(&value.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use serde_json::{Map, Value};
    use std::sync::Arc;

    use uuid::Uuid;

    use super::*;
    use crate::command::executor::checkout::ExecuteCheckout;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::checkout::CheckoutInput;
    use crate::command::input::transaction::TransactionInput;
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::{Entity, PropertyValue as PV};
    use crate::domain::validator::DomainValidator;
    use crate::store::storage::Storage;

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
              reasoning: { type: text, required: true }
            entity_change_meta:
              reasoning: { type: text, required: true }
            branch_meta:
              reasoning: { type: text, required: true }
        "})
            .unwrap(),
        )
    }

    fn meta(r: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(r.into()));
        m
    }

    fn exec(branch: &BranchContext, input: TransactionInput) {
        exec_tx(branch, input);
    }

    fn exec_tx(branch: &BranchContext, input: TransactionInput) -> Uuid {
        let cmd = ExecuteTransaction::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(branch.storage());
        let tx = cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
        tx.context.tx_id
    }

    fn grep(branch: &BranchContext, query: GrepEntitiesQuery) -> Vec<Entity<TxMeta>> {
        let cmd = GrepEntities::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, query).unwrap()
    }

    fn person(slug: &str, props: Vec<(&str, Value)>, reasoning: &str) -> Entity {
        Entity::new(
            slug.parse().unwrap(),
            Some(serde_json::json!("a person")),
            props
                .into_iter()
                .map(|(p, v)| PV {
                    property: p.parse().unwrap(),
                    value: v,
                    context: (),
                })
                .collect(),
            meta(reasoning),
        )
    }

    fn seed(branch: &BranchContext) {
        exec(
            branch,
            TransactionInput::new(meta("t1")).write_entity(person(
                "auth-service",
                vec![("role", serde_json::json!("authentication"))],
                "infra setup",
            )),
        );
        exec(
            branch,
            TransactionInput::new(meta("t2")).write_entity(person(
                "auth-gateway",
                vec![("region", serde_json::json!("eu"))],
                "edge node",
            )),
        );
        exec(
            branch,
            TransactionInput::new(meta("t3")).write_entity(person(
                "payment-service",
                vec![("currency", serde_json::json!("usd"))],
                "billing",
            )),
        );
    }

    #[test]
    fn matches_slug_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();
        seed(&branch);

        let results = grep(&branch, GrepEntitiesQuery::new("^auth-".into(), 50));
        let slugs: Vec<&str> = results.iter().map(|e| e.slug.as_str()).collect();
        assert_eq!(slugs, vec!["auth-gateway", "auth-service"]);
    }

    #[test]
    fn newest_first_and_limit() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();
        seed(&branch);

        // All three match ".*"; newest (payment, t3) first, limited to 2.
        let results = grep(&branch, GrepEntitiesQuery::new(".".into(), 2));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].slug.as_str(), "payment-service");
        assert_eq!(results[1].slug.as_str(), "auth-gateway");
    }

    #[test]
    fn properties_false_omits_properties() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();
        seed(&branch);

        let results = grep(
            &branch,
            GrepEntitiesQuery::new("auth-service".into(), 50).include_properties(false),
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].properties.is_empty());
        assert_eq!(results[0].description, Some(serde_json::json!("a person")));
    }

    #[test]
    fn matches_value_scope() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();
        seed(&branch);

        let scope = GrepScope {
            slug: false,
            value: true,
            ..GrepScope::default()
        };
        let results = grep(
            &branch,
            GrepEntitiesQuery::new("authentication".into(), 50).scope(scope),
        );
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].slug.as_str(), "auth-service");
    }

    #[test]
    fn matches_property_and_reasoning_scope() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();
        seed(&branch);

        let by_prop = grep(
            &branch,
            GrepEntitiesQuery::new("currency".into(), 50).scope(GrepScope {
                slug: false,
                property: true,
                ..GrepScope::default()
            }),
        );
        assert_eq!(by_prop.len(), 1);
        assert_eq!(by_prop[0].slug.as_str(), "payment-service");

        let by_reason = grep(
            &branch,
            GrepEntitiesQuery::new("billing".into(), 50).scope(GrepScope {
                slug: false,
                reasoning: true,
                ..GrepScope::default()
            }),
        );
        assert_eq!(by_reason.len(), 1);
        assert_eq!(by_reason[0].slug.as_str(), "payment-service");
    }

    #[test]
    fn invalid_regex_is_validation_error() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();
        let cmd = GrepEntities::new(test_schema());
        let mut state = CommandState::new(branch.storage());
        let err = cmd
            .execute(&branch, &mut state, GrepEntitiesQuery::new("(".into(), 50))
            .unwrap_err();
        assert!(matches!(
            err,
            DbError::Validation(ValidationError::InvalidRegex { .. })
        ));
    }

    #[test]
    fn deleted_entity_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();
        seed(&branch);

        exec(
            &branch,
            TransactionInput::new(meta("remove"))
                .delete_entity(crate::command::input::transaction::DeleteItem::new(
                    "auth-service".parse().unwrap(),
                    serde_json::json!("decommissioned"),
                )),
        );

        let results = grep(&branch, GrepEntitiesQuery::new("^auth-".into(), 50));
        let slugs: Vec<&str> = results.iter().map(|e| e.slug.as_str()).collect();
        assert_eq!(slugs, vec!["auth-gateway"]);
    }

    #[test]
    fn no_fields_matches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();
        seed(&branch);

        let scope = GrepScope {
            slug: false,
            property: false,
            value: false,
            reasoning: false,
        };
        let results = grep(&branch, GrepEntitiesQuery::new(".".into(), 50).scope(scope));
        assert!(results.is_empty());
    }

    #[test]
    fn at_tx_excludes_future_entity() {
        let dir = tempfile::tempdir().unwrap();
        let branch = Storage::open(dir.path(), test_config()).unwrap().main_branch();

        let tx1 = exec_tx(
            &branch,
            TransactionInput::new(meta("t1")).write_entity(person("alice", vec![], "first")),
        );
        // Created after tx1 — must not appear when querying at tx1.
        exec(
            &branch,
            TransactionInput::new(meta("t2")).write_entity(person("bob", vec![], "later")),
        );

        // Full-snapshot path (include_properties = true) is the one that used to
        // leak phantom entities for slugs created after at_tx.
        let results = grep(
            &branch,
            GrepEntitiesQuery::new(".".into(), 50).at_tx(tx1),
        );
        let slugs: Vec<&str> = results.iter().map(|e| e.slug.as_str()).collect();
        assert_eq!(slugs, vec!["alice"]);
    }

    #[test]
    fn inherits_from_parent_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec(
            &main,
            TransactionInput::new(meta("create")).write_entity(person(
                "auth-service",
                vec![("role", serde_json::json!("authentication"))],
                "infra",
            )),
        );

        let checkout = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(&storage);
        checkout
            .execute(
                &main,
                &mut state,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("seed child")),
                ),
            )
            .unwrap();
        state.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();

        // Found by slug from the parent, with provenance pointing at main.
        let by_slug = grep(&child, GrepEntitiesQuery::new("^auth-".into(), 50));
        assert_eq!(by_slug.len(), 1);
        assert_eq!(by_slug[0].slug.as_str(), "auth-service");
        assert_eq!(by_slug[0].context.branch.as_str(), "main");

        // And found by an inherited property value.
        let by_value = grep(
            &child,
            GrepEntitiesQuery::new("authentication".into(), 50).scope(GrepScope {
                slug: false,
                value: true,
                ..GrepScope::default()
            }),
        );
        assert_eq!(by_value.len(), 1);
        assert_eq!(by_value[0].slug.as_str(), "auth-service");
    }

    #[test]
    fn deletion_on_child_shadows_parent() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec(
            &main,
            TransactionInput::new(meta("create")).write_entity(person("auth-service", vec![], "infra")),
        );

        let checkout = ExecuteCheckout::new(DomainValidator::new(test_schema()));
        let mut state = CommandState::new(&storage);
        checkout
            .execute(
                &main,
                &mut state,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("drop it")).delete_entity(
                        crate::command::input::transaction::DeleteItem::new(
                            "auth-service".parse().unwrap(),
                            serde_json::json!("not relevant here"),
                        ),
                    ),
                ),
            )
            .unwrap();
        state.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        // Deleted on child → excluded there, even though it lives on the parent.
        assert!(grep(&child, GrepEntitiesQuery::new("^auth-".into(), 50)).is_empty());
        // Still present on the parent branch.
        let on_main = grep(&main, GrepEntitiesQuery::new("^auth-".into(), 50));
        assert_eq!(on_main.len(), 1);
    }
}
