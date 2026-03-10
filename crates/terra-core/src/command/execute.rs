use crate::assertion::{AssertionStore, ItemKind};
use crate::schema::BranchSchemaRegistry;

use super::{Command, CommandError, CommandResult};
use super::assert_entity;
use super::branch;

/// Executes a domain command against the schema registry and assertion store.
pub fn execute(
    cmd: Command,
    registry: &BranchSchemaRegistry,
    store: &AssertionStore,
) -> Result<CommandResult, CommandError> {
    match cmd {
        Command::ListEntityTypes => {
            let types = registry.list_entity_types()?;
            let vis = store.visibility();
            let ancestry = registry.ancestry();
            let types = types
                .into_iter()
                .filter(|t| vis.is_visible(ancestry, ItemKind::EntityType, t.id).unwrap_or(true))
                .collect();
            Ok(CommandResult::EntityTypes(types))
        }
        Command::ListProperties {
            entity_type: None,
        } => {
            let props = registry.list_all_properties()?;
            let vis = store.visibility();
            let ancestry = registry.ancestry();
            let props = props
                .into_iter()
                .filter(|p| vis.is_visible(ancestry, ItemKind::Property, p.id).unwrap_or(true))
                .collect();
            Ok(CommandResult::Properties(props))
        }
        Command::ListProperties {
            entity_type: Some(et),
        } => {
            let props = registry.list_properties(&et)?;
            let vis = store.visibility();
            let ancestry = registry.ancestry();
            let props = props
                .into_iter()
                .filter(|p| vis.is_visible(ancestry, ItemKind::Property, p.id).unwrap_or(true))
                .collect();
            Ok(CommandResult::Properties(props))
        }
        Command::Transaction(input) => {
            let result = assert_entity::execute_transaction(input, registry, store)?;
            Ok(CommandResult::TransactionResult {
                transaction: result.transaction,
                entity_types: result.entity_types,
                properties: result.properties,
                attached_count: result.attached_count,
                introduced: result.introduced,
                asserted: result.asserted,
            })
        }
        Command::ListEntities => {
            let entities = store.entities().list_active()?;
            let vis = store.visibility();
            let ancestry = registry.ancestry();
            let entities = entities
                .into_iter()
                .filter(|e| vis.is_visible(ancestry, ItemKind::Entity, e.id).unwrap_or(true))
                .collect();
            Ok(CommandResult::EntityList(entities))
        }
        Command::CreateBranch(input) => {
            let detail = branch::create_branch(input, store)?;
            Ok(CommandResult::Branch(detail))
        }
        Command::GetBranch { slug } => {
            let detail = branch::get_branch(&slug, store)?;
            Ok(CommandResult::Branch(detail))
        }
        Command::ListBranches => {
            let list = branch::list_branches(store)?;
            Ok(CommandResult::BranchList(list))
        }
        Command::ListLog => {
            let entries = store.facts().list()?;
            Ok(CommandResult::LogEntries(entries))
        }
        Command::BranchState { slug, last_transactions, at_tx } => {
            let state = super::branch_state::build_state(&slug, last_transactions, at_tx, registry, store)?;
            Ok(CommandResult::BranchState(state))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{
        AssertionItem, CreateEntityType, CreateProperty, HideUnhideInput,
        IntroduceItem, TransactionInput,
    };
    use crate::assertion::{PropertyValue, RangeValue, MAIN_BRANCH};
    use crate::schema::ValueType;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn setup() -> (BranchSchemaRegistry, AssertionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        let registry = store.schema_registry(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);
        (registry, store, dir)
    }

    fn setup_schema(reg: &BranchSchemaRegistry, store: &AssertionStore) {
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![],
                properties: vec![
                    CreateProperty {
                        slug: "bpm".into(),
                        value_type: ValueType::Range,
                        description: None,
                        entity_types: vec![],
                    },
                    CreateProperty {
                        slug: "certification".into(),
                        value_type: ValueType::Set,
                        description: None,
                        entity_types: vec![],
                    },
                ],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            reg,
            store,
        )
        .unwrap();

        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![CreateEntityType {
                    slug: "track".into(),
                    description: None,
                    properties: vec!["bpm".into(), "certification".into()],
                }],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            reg,
            store,
        )
        .unwrap();
    }

    #[test]
    fn create_and_list_entity_types() {
        let (reg, store, _dir) = setup();

        let result = execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![CreateEntityType {
                    slug: "unit".into(),
                    description: Some("Research project".into()),
                    properties: vec![],
                }],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::TransactionResult { entity_types, .. } => {
                assert_eq!(entity_types.len(), 1);
                assert_eq!(entity_types[0].slug, "unit");
            }
            _ => panic!("unexpected result"),
        }

        let result = execute(Command::ListEntityTypes, &reg, &store).unwrap();
        match result {
            CommandResult::EntityTypes(types) => assert_eq!(types.len(), 1),
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn create_entity_via_transaction() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        let result = execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("initial setup"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![IntroduceItem {
                    entity: "song-1".into(),
                    description: Some("A test song".into()),
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(serde_json::json!(128))),
                        )]),
                        reasoning: serde_json::json!("detected"),
                    }],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        match result {
            CommandResult::TransactionResult {
                introduced,
                ..
            } => {
                assert_eq!(introduced.len(), 1);
                assert_eq!(introduced[0].facts.len(), 1);
                assert!(introduced[0].hypotheses.is_empty());
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn list_entities_after_create() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![IntroduceItem {
                    entity: "alpha".into(),
                    description: None,
                    facts: vec![],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        let result = execute(Command::ListEntities, &reg, &store).unwrap();
        match result {
            CommandResult::EntityList(entities) => {
                assert_eq!(entities.len(), 1);
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn hidden_entity_type_excluded_from_list() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        // Hide entity type "track"
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("hide track"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput {
                    entities: vec![],
                    entity_types: vec!["track".into()],
                    properties: vec![],
                },
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        // ListEntityTypes should not include "track"
        let result = execute(Command::ListEntityTypes, &reg, &store).unwrap();
        match result {
            CommandResult::EntityTypes(types) => {
                assert!(types.iter().all(|t| t.slug != "track"));
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn hidden_property_excluded_from_list() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        // Hide property "bpm"
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("hide bpm"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput {
                    entities: vec![],
                    entity_types: vec![],
                    properties: vec!["bpm".into()],
                },
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        // ListProperties for "track" should not include "bpm"
        let result = execute(
            Command::ListProperties { entity_type: Some("track".into()) },
            &reg,
            &store,
        )
        .unwrap();
        match result {
            CommandResult::Properties(props) => {
                assert_eq!(props.len(), 1);
                assert_eq!(props[0].slug, "certification");
            }
            _ => panic!("unexpected result"),
        }

        // ListProperties (all) should also exclude "bpm"
        let result = execute(
            Command::ListProperties { entity_type: None },
            &reg,
            &store,
        )
        .unwrap();
        match result {
            CommandResult::Properties(props) => {
                assert!(props.iter().all(|p| p.slug != "bpm"));
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn hidden_entity_excluded_from_list() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        // Create two entities
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![
                    IntroduceItem { entity: "alpha".into(), description: None, facts: vec![], hypotheses: vec![] },
                    IntroduceItem { entity: "beta".into(), description: None, facts: vec![], hypotheses: vec![] },
                ],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        // Hide "alpha"
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("hide alpha"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput {
                    entities: vec!["alpha".into()],
                    entity_types: vec![],
                    properties: vec![],
                },
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            &reg,
            &store,
        )
        .unwrap();

        let result = execute(Command::ListEntities, &reg, &store).unwrap();
        match result {
            CommandResult::EntityList(entities) => {
                assert_eq!(entities.len(), 1);
                assert_eq!(entities[0].slug, "beta");
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn branch_state_empty_branch() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        let result = execute(
            Command::BranchState { slug: "main".into(), last_transactions: 10, at_tx: None },
            &reg,
            &store,
        ).unwrap();

        match result {
            CommandResult::BranchState(state) => {
                assert_eq!(state.branch.slug, "main");
                assert!(!state.schema.entity_types.is_empty());
                assert!(!state.schema.properties.is_empty());
                assert!(state.entities.is_empty());
                // setup_schema creates 2 transactions
                assert!(!state.recent_transactions.is_empty());
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn branch_state_with_fact_and_reasoning() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("initial analysis"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![IntroduceItem {
                    entity: "song-1".into(),
                    description: Some("A pop track".into()),
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(serde_json::json!(128))),
                        )]),
                        reasoning: serde_json::json!("detected via audio analysis"),
                    }],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            }),
            &reg,
            &store,
        ).unwrap();

        let result = execute(
            Command::BranchState { slug: "main".into(), last_transactions: 10, at_tx: None },
            &reg,
            &store,
        ).unwrap();

        match result {
            CommandResult::BranchState(state) => {
                assert_eq!(state.entities.len(), 1);
                let entity = &state.entities[0];
                assert_eq!(entity.slug, "song-1");
                assert_eq!(entity.description.as_deref(), Some("A pop track"));
                assert_eq!(entity.types.len(), 1);
                assert_eq!(entity.types[0].entity_type, "track");

                let bpm = entity.types[0].properties.iter().find(|p| p.slug == "bpm").unwrap();
                assert!(bpm.fact.is_some());
                let fact = bpm.fact.as_ref().unwrap();
                assert_eq!(fact.value, serde_json::json!({"eq": 128}));
                assert_eq!(fact.reasoning, serde_json::json!("detected via audio analysis"));
                assert!(bpm.hypotheses.is_empty());
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn branch_state_with_hypotheses() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        // Introduce with a fact
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("initial"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![IntroduceItem {
                    entity: "song-2".into(),
                    description: None,
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(serde_json::json!(120))),
                        )]),
                        reasoning: serde_json::json!("first measurement"),
                    }],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            }),
            &reg,
            &store,
        ).unwrap();

        // Add hypotheses
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("re-analysis"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![crate::command::AssertItem {
                    entity: "song-2".into(),
                    facts: vec![],
                    hypotheses: vec![
                        AssertionItem {
                            entity_type: "track".into(),
                            properties: HashMap::from([(
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(serde_json::json!(122))),
                            )]),
                            reasoning: serde_json::json!("maybe higher"),
                        },
                        AssertionItem {
                            entity_type: "track".into(),
                            properties: HashMap::from([(
                                "bpm".into(),
                                PropertyValue::Range(RangeValue::Eq(serde_json::json!(118))),
                            )]),
                            reasoning: serde_json::json!("maybe lower"),
                        },
                    ],
                }],
            }),
            &reg,
            &store,
        ).unwrap();

        let result = execute(
            Command::BranchState { slug: "main".into(), last_transactions: 10, at_tx: None },
            &reg,
            &store,
        ).unwrap();

        match result {
            CommandResult::BranchState(state) => {
                let entity = &state.entities[0];
                let bpm = entity.types[0].properties.iter().find(|p| p.slug == "bpm").unwrap();
                assert!(bpm.fact.is_some());
                assert_eq!(bpm.hypotheses.len(), 2);
                // Check hypothesis values and reasoning
                assert_eq!(bpm.hypotheses[0].value, serde_json::json!({"eq": 122}));
                assert_eq!(bpm.hypotheses[0].reasoning, serde_json::json!("maybe higher"));
                assert_eq!(bpm.hypotheses[1].value, serde_json::json!({"eq": 118}));
                assert_eq!(bpm.hypotheses[1].reasoning, serde_json::json!("maybe lower"));
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn branch_state_respects_visibility() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        // Create two entities
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![
                    IntroduceItem { entity: "visible".into(), description: None, facts: vec![], hypotheses: vec![] },
                    IntroduceItem { entity: "hidden".into(), description: None, facts: vec![], hypotheses: vec![] },
                ],
                asserts: vec![],
            }),
            &reg,
            &store,
        ).unwrap();

        // Hide one entity and bpm property
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("hide stuff"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput {
                    entities: vec!["hidden".into()],
                    entity_types: vec![],
                    properties: vec!["bpm".into()],
                },
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            &reg,
            &store,
        ).unwrap();

        let result = execute(
            Command::BranchState { slug: "main".into(), last_transactions: 10, at_tx: None },
            &reg,
            &store,
        ).unwrap();

        match result {
            CommandResult::BranchState(state) => {
                // Only "visible" entity should appear
                assert_eq!(state.entities.len(), 1);
                assert_eq!(state.entities[0].slug, "visible");

                // "bpm" should not be in properties
                assert!(state.schema.properties.iter().all(|p| p.slug != "bpm"));

                // "bpm" should not be in entity type's attached properties
                let track = state.schema.entity_types.iter().find(|t| t.slug == "track").unwrap();
                assert!(!track.properties.contains(&"bpm".to_string()));
                assert!(track.properties.contains(&"certification".to_string()));
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn branch_state_skips_types_without_assertions() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        // Add another entity type with no assertions
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!(null),
                entity_types: vec![CreateEntityType {
                    slug: "album".into(),
                    description: None,
                    properties: vec![],
                }],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![IntroduceItem {
                    entity: "song-x".into(),
                    description: None,
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(serde_json::json!(100))),
                        )]),
                        reasoning: serde_json::json!("test"),
                    }],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            }),
            &reg,
            &store,
        ).unwrap();

        let result = execute(
            Command::BranchState { slug: "main".into(), last_transactions: 10, at_tx: None },
            &reg,
            &store,
        ).unwrap();

        match result {
            CommandResult::BranchState(state) => {
                let entity = &state.entities[0];
                // Only "track" type should appear (has assertions), not "album"
                assert_eq!(entity.types.len(), 1);
                assert_eq!(entity.types[0].entity_type, "track");
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn branch_state_recent_transactions_with_limit() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        // setup_schema creates 2 transactions; add 3 more
        for i in 0..3 {
            execute(
                Command::Transaction(TransactionInput {
                    reasoning: serde_json::json!(format!("tx-{}", i)),
                    entity_types: vec![],
                    properties: vec![],
                    attach: vec![],
                    hide: HideUnhideInput::default(),
                    unhide: HideUnhideInput::default(),
                    introduce: vec![],
                    asserts: vec![],
                }),
                &reg,
                &store,
            ).unwrap();
        }

        // Get with limit 2
        let result = execute(
            Command::BranchState { slug: "main".into(), last_transactions: 2, at_tx: None },
            &reg,
            &store,
        ).unwrap();

        match result {
            CommandResult::BranchState(state) => {
                assert_eq!(state.recent_transactions.len(), 2);
                // Newest first
                assert_eq!(state.recent_transactions[0].reasoning, serde_json::json!("tx-2"));
                assert_eq!(state.recent_transactions[1].reasoning, serde_json::json!("tx-1"));
            }
            _ => panic!("unexpected result"),
        }
    }

    #[test]
    fn branch_state_at_tx_time_travel() {
        let (reg, store, _dir) = setup();
        setup_schema(&reg, &store);

        // TX1: introduce entity with a fact
        let result = execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("first measurement"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![IntroduceItem {
                    entity: "song-tt".into(),
                    description: None,
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(serde_json::json!(120))),
                        )]),
                        reasoning: serde_json::json!("initial"),
                    }],
                    hypotheses: vec![],
                }],
                asserts: vec![],
            }),
            &reg,
            &store,
        ).unwrap();

        let tx1_id = match &result {
            CommandResult::TransactionResult { transaction, .. } => transaction.id,
            _ => panic!("unexpected"),
        };

        // TX2: update the fact
        execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("corrected measurement"),
                entity_types: vec![],
                properties: vec![],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![crate::command::AssertItem {
                    entity: "song-tt".into(),
                    facts: vec![AssertionItem {
                        entity_type: "track".into(),
                        properties: HashMap::from([(
                            "bpm".into(),
                            PropertyValue::Range(RangeValue::Eq(serde_json::json!(125))),
                        )]),
                        reasoning: serde_json::json!("corrected"),
                    }],
                    hypotheses: vec![],
                }],
            }),
            &reg,
            &store,
        ).unwrap();

        // State at HEAD — should see bpm=125
        let result = execute(
            Command::BranchState { slug: "main".into(), last_transactions: 10, at_tx: None },
            &reg,
            &store,
        ).unwrap();
        match &result {
            CommandResult::BranchState(state) => {
                let bpm = &state.entities[0].types[0].properties.iter()
                    .find(|p| p.slug == "bpm").unwrap();
                assert_eq!(bpm.fact.as_ref().unwrap().value, serde_json::json!({"eq": 125}));
            }
            _ => panic!("unexpected"),
        }

        // State at TX1 — should see bpm=120
        let result = execute(
            Command::BranchState { slug: "main".into(), last_transactions: 10, at_tx: Some(tx1_id) },
            &reg,
            &store,
        ).unwrap();
        match &result {
            CommandResult::BranchState(state) => {
                let bpm = &state.entities[0].types[0].properties.iter()
                    .find(|p| p.slug == "bpm").unwrap();
                assert_eq!(bpm.fact.as_ref().unwrap().value, serde_json::json!({"eq": 120}));
                assert_eq!(bpm.fact.as_ref().unwrap().reasoning, serde_json::json!("initial"));
                // TX2 should not be in recent transactions
                assert!(state.recent_transactions.iter().all(|t| t.reasoning != serde_json::json!("corrected measurement")));
            }
            _ => panic!("unexpected"),
        }
    }

    #[test]
    fn transaction_creates_properties_before_entity_types() {
        let (reg, store, _dir) = setup();

        // Single transaction: entity_types reference properties defined in the same transaction.
        // Properties must be processed first even though entity_types appear first in the input.
        let result = execute(
            Command::Transaction(TransactionInput {
                reasoning: serde_json::json!("bootstrap schema in one go"),
                entity_types: vec![CreateEntityType {
                    slug: "track".into(),
                    description: None,
                    properties: vec!["bpm".into()],
                }],
                properties: vec![CreateProperty {
                    slug: "bpm".into(),
                    value_type: ValueType::Range,
                    description: None,
                    entity_types: vec![],
                }],
                attach: vec![],
                hide: HideUnhideInput::default(),
                unhide: HideUnhideInput::default(),
                introduce: vec![],
                asserts: vec![],
            }),
            &reg,
            &store,
        );

        assert!(result.is_ok(), "expected success but got: {:?}", result.err());
        match result.unwrap() {
            CommandResult::TransactionResult { entity_types, properties, .. } => {
                assert_eq!(entity_types.len(), 1);
                assert_eq!(entity_types[0].slug, "track");
                assert_eq!(properties.len(), 1);
                assert_eq!(properties[0].slug, "bpm");
            }
            _ => panic!("unexpected"),
        }
    }
}
