//! TransactionInput — command describing an atomic mutation.
//!
//! Built incrementally by the caller, then passed to ExecuteTransaction.
//! All fields are private — populated through builder methods.

use serde_json::{Map, Value};

use crate::domain::entity::Entity;
use crate::domain::managed::Managed;

/// Atomic mutation command — all operations to execute in a single transaction.
///
/// Created via `TransactionInput::new(meta)`, then populated with
/// builder methods. Passed to `ExecuteTransaction` for execution.
///
/// Processing order (enforced by executor, not by input):
/// 1. Create entities
/// 2. Update entities (assertions on existing)
/// 3. Create managed items
/// 4. Update managed items
pub struct TransactionInput {
    pub(crate) meta: Map<String, Value>,
    pub(crate) create_entities: Vec<Entity>,
    pub(crate) update_entities: Vec<Entity>,
    pub(crate) create_managed: Vec<Managed>,
    pub(crate) update_managed: Vec<Managed>,
}

impl TransactionInput {
    /// Start building a transaction with the given metadata.
    pub fn new(meta: Map<String, Value>) -> Self {
        Self {
            meta,
            create_entities: Vec::new(),
            update_entities: Vec::new(),
            create_managed: Vec::new(),
            update_managed: Vec::new(),
        }
    }

    /// Add a new entity to create.
    pub fn create_entity(mut self, entity: Entity) -> Self {
        self.create_entities.push(entity);
        self
    }

    /// Add an existing entity to update.
    pub fn update_entity(mut self, entity: Entity) -> Self {
        self.update_entities.push(entity);
        self
    }

    /// Add a new managed item to create.
    pub fn create_managed(mut self, managed: Managed) -> Self {
        self.create_managed.push(managed);
        self
    }

    /// Add an existing managed item to update.
    pub fn update_managed(mut self, managed: Managed) -> Self {
        self.update_managed.push(managed);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entity::PropertyValue;

    fn meta(reasoning: &str) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert("reasoning".into(), Value::String(reasoning.into()));
        m
    }

    #[test]
    fn empty_transaction() {
        let input = TransactionInput::new(meta("test"));
        assert_eq!(input.meta["reasoning"], "test");
        assert!(input.create_entities.is_empty());
        assert!(input.update_entities.is_empty());
        assert!(input.create_managed.is_empty());
        assert!(input.update_managed.is_empty());
    }

    #[test]
    fn with_entities() {
        let input = TransactionInput::new(meta("add entities"))
            .create_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            ))
            .update_entity(Entity::new(
                "bob".parse().unwrap(),
                None,
                vec![PropertyValue { property: "age".parse().unwrap(), value: serde_json::json!(30), context: () }],
                Map::new(),
            ));

        assert_eq!(input.create_entities.len(), 1);
        assert_eq!(input.create_entities[0].slug.as_str(), "alice");
        assert_eq!(input.update_entities.len(), 1);
        assert_eq!(input.update_entities[0].slug.as_str(), "bob");
    }

    #[test]
    fn with_managed() {
        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("do stuff"));

        let input = TransactionInput::new(meta("manage"))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("open".into()),
                fields,
            ))
            .update_managed(Managed::new(
                "task".parse().unwrap(),
                "task-1".parse().unwrap(),
                Some("closed".into()),
                Map::new(),
            ));

        assert_eq!(input.create_managed.len(), 1);
        assert_eq!(input.create_managed[0].slug.as_str(), "task-1");
        assert_eq!(input.update_managed.len(), 1);
        assert_eq!(input.update_managed[0].state.as_deref(), Some("closed"));
    }

    #[test]
    fn mixed_operations() {
        let mut fields = Map::new();
        fields.insert("goal".into(), serde_json::json!("investigate"));

        let input = TransactionInput::new(meta("mixed"))
            .create_entity(Entity::new(
                "server".parse().unwrap(),
                Some(serde_json::json!("Production server")),
                vec![],
                Map::new(),
            ))
            .update_entity(Entity::new(
                "server".parse().unwrap(),
                None,
                vec![PropertyValue { property: "status".parse().unwrap(), value: serde_json::json!("down"), context: () }],
                Map::new(),
            ))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "fix-server".parse().unwrap(),
                Some("open".into()),
                fields,
            ));

        assert_eq!(input.create_entities.len(), 1);
        assert_eq!(input.update_entities.len(), 1);
        assert_eq!(input.create_managed.len(), 1);
        assert!(input.update_managed.is_empty());
    }
}
