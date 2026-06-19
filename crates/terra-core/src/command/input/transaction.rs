//! TransactionInput — command describing an atomic mutation.
//!
//! Built incrementally by the caller, then passed to ExecuteTransaction.
//! All fields are private — populated through builder methods.

use serde_json::{Map, Value};

use crate::domain::entity::Entity;
use crate::domain::managed::Managed;
use crate::io::slug::Slug;

/// Explicit touch — agent declares an entity as relevant to this transaction.
pub struct TouchItem {
    pub(crate) entity: Slug,
    pub(crate) reasoning: String,
}

impl TouchItem {
    pub fn new(entity: Slug, reasoning: impl Into<String>) -> Self {
        Self {
            entity,
            reasoning: reasoning.into(),
        }
    }
}

/// Soft-delete an entity — marks it as deleted with reasoning.
pub struct DeleteItem {
    pub(crate) entity: Slug,
    pub(crate) reasoning: Value,
}

impl DeleteItem {
    pub fn new(entity: Slug, reasoning: Value) -> Self {
        Self { entity, reasoning }
    }
}

/// Atomic mutation command — all operations to execute in a single transaction.
///
/// Created via `TransactionInput::new(meta)`, then populated with
/// builder methods. Passed to `ExecuteTransaction` for execution.
///
/// Processing order (enforced by executor, not by input):
/// 1. Write entities (create if new, update if existing)
/// 2. Create managed items
/// 3. Update managed items
/// 4. Delete entities (soft-delete with reasoning)
/// 5. Explicit touches (override auto-touches from writes)
pub struct TransactionInput {
    pub(crate) meta: Map<String, Value>,
    pub(crate) write_entities: Vec<Entity>,
    pub(crate) create_managed: Vec<Managed>,
    pub(crate) update_managed: Vec<Managed>,
    pub(crate) delete_entities: Vec<DeleteItem>,
    pub(crate) touched: Vec<TouchItem>,
}

impl TransactionInput {
    /// Start building a transaction with the given metadata.
    pub fn new(meta: Map<String, Value>) -> Self {
        Self {
            meta,
            write_entities: Vec::new(),
            create_managed: Vec::new(),
            update_managed: Vec::new(),
            delete_entities: Vec::new(),
            touched: Vec::new(),
        }
    }

    /// Add an entity to write — created if new, updated if it already exists.
    /// A description is required only when the entity is new.
    pub fn write_entity(mut self, entity: Entity) -> Self {
        self.write_entities.push(entity);
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

    /// Soft-delete an entity.
    pub fn delete_entity(mut self, item: DeleteItem) -> Self {
        self.delete_entities.push(item);
        self
    }

    /// Explicitly mark an entity as relevant to this transaction.
    /// Overrides auto-touch reasoning from create/update if the same entity.
    pub fn touch(mut self, item: TouchItem) -> Self {
        self.touched.push(item);
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
        assert!(input.write_entities.is_empty());
        assert!(input.write_entities.is_empty());
        assert!(input.create_managed.is_empty());
        assert!(input.update_managed.is_empty());
    }

    #[test]
    fn with_entities() {
        let input = TransactionInput::new(meta("add entities"))
            .write_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![],
                Map::new(),
            ))
            .write_entity(Entity::new(
                "bob".parse().unwrap(),
                None,
                vec![PropertyValue {
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(30),
                    context: (),
                }],
                Map::new(),
            ));

        assert_eq!(input.write_entities.len(), 2);
        assert_eq!(input.write_entities[0].slug.as_str(), "alice");
        assert_eq!(input.write_entities[1].slug.as_str(), "bob");
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
            .write_entity(Entity::new(
                "server".parse().unwrap(),
                Some(serde_json::json!("Production server")),
                vec![],
                Map::new(),
            ))
            .write_entity(Entity::new(
                "server".parse().unwrap(),
                None,
                vec![PropertyValue {
                    property: "status".parse().unwrap(),
                    value: serde_json::json!("down"),
                    context: (),
                }],
                Map::new(),
            ))
            .create_managed(Managed::new(
                "task".parse().unwrap(),
                "fix-server".parse().unwrap(),
                Some("open".into()),
                fields,
            ));

        assert_eq!(input.write_entities.len(), 2);
        assert_eq!(input.create_managed.len(), 1);
        assert!(input.update_managed.is_empty());
    }

    #[test]
    fn with_touch() {
        let input = TransactionInput::new(meta("observe"))
            .touch(TouchItem::new("alice".parse().unwrap(), "key witness"))
            .touch(TouchItem::new("server".parse().unwrap(), "infrastructure"));

        assert_eq!(input.touched.len(), 2);
        assert_eq!(input.touched[0].entity.as_str(), "alice");
        assert_eq!(input.touched[0].reasoning, "key witness");
        assert_eq!(input.touched[1].entity.as_str(), "server");
    }
}
