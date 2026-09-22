//! Entity property queries — latest assertion per property with ancestry walk.

use crate::domain::property_path;
use std::collections::{BTreeSet, HashSet};

use uuid::Uuid;

use crate::config::AssertionStatusesDef;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::DbError;
use crate::store::branch_context::BranchContext;
use crate::store::entry::assertion::{AssertionEntry, AssertionKey, AssertionRange};

/// Get the latest assertion per property for an entity, walking the ancestry chain.
///
/// If `at_tx` is Some, only assertions up to that tx_id are considered.
/// Results are sorted by property slug (stable alphabetical order).
pub fn properties(
    branch: &BranchContext,
    entity: &Slug,
    at_tx: Option<Uuid>,
) -> Result<Vec<AssertionEntry>, DbError> {
    // This API promises one latest value per property, including for CAS callers.
    let mut latest = std::collections::BTreeMap::new();
    for entry in visible_properties(branch, entity, at_tx, None, &[], None)? {
        let current = latest
            .entry(entry.key.prop.clone())
            .or_insert_with(|| entry.clone());
        if entry.key.tx_id > current.key.tx_id {
            *current = entry;
        }
    }
    Ok(latest.into_values().collect())
}

/// Status-aware snapshot of an entity's properties.
///
/// For each property: the latest terminal assertion forms the baseline, and every
/// non-terminal assertion made *after* it (hypotheses, observations) is layered on
/// top. Anything older than the latest terminal is consolidated away — a terminal
/// assertion resets the picture. Returns a flat list: per property (sorted by slug)
/// the baseline first (if present), then overlays newest-first. Retracted (null)
/// values are omitted; a property whose only baseline is a retraction yields just
/// its later overlays, or nothing.
///
/// Scope streams are merged newest-first until the latest non-withdrawn terminal.
/// Addressed operations invalidate their targets before cutoff selection. Reopening
/// a summary can therefore read across multiple former terminal boundaries; without
/// a remaining terminal the full visible property history is traversed.
pub fn layered_properties(
    branch: &BranchContext,
    entity: &Slug,
    at_tx: Option<Uuid>,
    statuses: &AssertionStatusesDef,
) -> Result<Vec<AssertionEntry>, DbError> {
    visible_properties(branch, entity, at_tx, Some(statuses), &[], None)
}

fn visible_properties(
    branch: &BranchContext,
    entity: &Slug,
    at_tx: Option<Uuid>,
    statuses: Option<&AssertionStatusesDef>,
    pending: &[AssertionEntry],
    selected: Option<&BTreeSet<Slug>>,
) -> Result<Vec<AssertionEntry>, DbError> {
    let scopes: Vec<_> = match at_tx {
        Some(tx) => branch.scopes_at(tx).collect(),
        None => branch.scopes().collect(),
    };

    let mut props: BTreeSet<Slug> = BTreeSet::new();
    if let Some(selected) = selected {
        props.extend(selected.iter().cloned());
    } else {
        for scope in &scopes {
            discover_props(branch, entity, &scope.branch, &mut props)?;
        }
    }
    let mut staged_by_property = std::collections::BTreeMap::<Slug, Vec<AssertionEntry>>::new();
    for entry in pending {
        staged_by_property
            .entry(entry.key.prop.clone())
            .or_default()
            .push(entry.clone());
    }
    props.extend(staged_by_property.keys().cloned());
    let mut result = Vec::new();
    for prop in props {
        // Merge scope streams by transaction order. Child retractions can invalidate
        // inherited terminal boundaries; never stop at one before processing newer edits.
        let mut streams: Vec<Box<dyn Iterator<Item = Result<AssertionEntry, DbError>> + '_>> =
            Vec::new();
        for scope in &scopes {
            let mut bound = AssertionKey::bound().with_prefix(|k| {
                k.branch = scope.branch.clone();
                k.entity = entity.clone();
                k.prop = prop.clone();
            });
            if let Some(upper) = scope.upper_tx {
                bound = bound.with_upper(|k| k.tx_id = upper);
            }
            streams.push(Box::new(
                branch.storage().scan_rev::<AssertionEntry>(&bound)?,
            ));
        }
        let mut staged = staged_by_property.remove(&prop).unwrap_or_default();
        staged.sort_by_key(|p| std::cmp::Reverse(p.key.tx_id));
        streams.push(Box::new(staged.into_iter().map(Ok)));
        let mut heads: Vec<_> = streams
            .iter_mut()
            .map(|s| s.next().transpose())
            .collect::<Result<_, _>>()?;
        let mut replaced = HashSet::new();
        let mut overlays = Vec::new();
        loop {
            let next = heads
                .iter()
                .enumerate()
                .filter_map(|(i, h)| h.as_ref().map(|a| (i, a.key.tx_id)))
                .max_by_key(|(_, tx)| *tx);
            let Some((i, _)) = next else {
                break;
            };
            let entry = heads[i].take().unwrap();
            if let Some(target) = entry.value.supersedes_tx {
                replaced.insert(target);
            }
            let hidden = replaced.contains(&entry.key.tx_id);
            let terminal = entry.value.supersedes_tx.is_none()
                && statuses.is_none_or(|s| s.is_terminal(entry.value.status.as_deref()));
            // In the legacy statusless view, preserve last-value semantics. With
            // statuses, withdrawing a terminal removes its cutoff as well as its value.
            if terminal && (!hidden || statuses.is_none()) {
                if !hidden && !entry.value.is_deleted() {
                    result.push(entry);
                }
                break;
            }
            if !hidden && !entry.value.is_deleted() {
                overlays.push(entry);
            }
            heads[i] = streams[i].next().transpose()?;
        }
        result.extend(overlays);
    }

    Ok(result)
}

/// Resolve one exact property without discovering/reading its siblings.
/// Uses the same historical and supersession policy as the full snapshot.
pub(crate) fn exact_property(
    branch: &BranchContext,
    entity: &Slug,
    property: &Slug,
    statuses: Option<&AssertionStatusesDef>,
) -> Result<Vec<AssertionEntry>, DbError> {
    visible_properties(
        branch,
        entity,
        None,
        statuses,
        &[],
        Some(&BTreeSet::from([property.clone()])),
    )
}

/// Select a contiguous path range before resolving assertion histories.
/// Depth boundaries seek past omitted subtrees without decoding their values.
/// Refs describe structural paths; opening one may yield no live assertions.
pub(crate) fn selected_properties(
    branch: &BranchContext,
    entity: &Slug,
    at_tx: Option<Uuid>,
    statuses: Option<&AssertionStatusesDef>,
    prefix: Option<&Slug>,
    depth: Option<usize>,
) -> Result<(Vec<AssertionEntry>, Vec<Slug>), DbError> {
    let scopes: Vec<_> = match at_tx {
        Some(tx) => branch.scopes_at(tx).collect(),
        None => branch.scopes().collect(),
    };
    let mut selected = BTreeSet::new();
    let mut refs = BTreeSet::new();
    for scope in scopes {
        let bound = AssertionRange::subtree(&scope.branch, entity, prefix.map(Slug::as_str));
        let mut iter = branch.storage().scan_keys::<AssertionEntry>(&bound)?;
        while let Some(key) = iter.next() {
            let key = key?;
            // The first version is the oldest: a future-only property isn't in this snapshot.
            if scope.upper_tx.is_none_or(|upper| key.tx_id <= upper) {
                let path = key.prop.as_str();
                let relative = match prefix {
                    Some(p) if path == p.as_str() => Some(""),
                    Some(p) => property_path::relative(path, p.as_str()),
                    None => Some(path),
                };
                if let Some(relative) = relative {
                    let exact = prefix.is_some_and(|p| p.as_str() == path);
                    let n = if exact {
                        0
                    } else {
                        property_path::depth(relative)
                    };
                    if depth.is_none_or(|d| n <= d) {
                        selected.insert(key.prop.clone());
                    } else {
                        let d = depth.unwrap();
                        let base = prefix.map_or(0, |p| property_path::depth(p.as_str()));
                        let count = (base + d.min(n)).max(1);
                        let target = property_path::prefix(path, count);
                        // A valid full slug can have a non-addressable prefix (empty or trailing hyphen).
                        let target = target.parse::<Slug>().unwrap_or_else(|_| key.prop.clone());
                        iter.seek(&AssertionRange::after_subtree(
                            &scope.branch,
                            entity,
                            target.as_str(),
                        ));
                        refs.insert(target);
                        continue;
                    }
                }
            }
            let skip = AssertionKey::bound().with_prefix(|k| {
                k.branch = scope.branch.clone();
                k.entity = entity.clone();
                k.prop = key.prop.clone();
                k.tx_id = Uuid::max();
            });
            iter.seek(&skip);
        }
    }
    let entries = visible_properties(branch, entity, at_tx, statuses, &[], Some(&selected))?;
    let entries = if statuses.is_none() {
        let mut latest = std::collections::BTreeMap::new();
        for entry in entries {
            let current = latest
                .entry(entry.key.prop.clone())
                .or_insert_with(|| entry.clone());
            if entry.key.tx_id > current.key.tx_id {
                *current = entry;
            }
        }
        latest.into_values().collect()
    } else {
        entries
    };
    Ok((entries, refs.into_iter().collect()))
}

/// Preview a transaction's assertions using the same visibility rules as committed reads.
/// Used to keep embeddings consistent when a terminal withdrawal exposes older evidence.
pub(crate) fn with_pending(
    branch: &BranchContext,
    entity: &Slug,
    statuses: Option<&AssertionStatusesDef>,
    pending: &[AssertionEntry],
) -> Result<Vec<AssertionEntry>, DbError> {
    visible_properties(branch, entity, None, statuses, pending, None)
}

/// Discover the distinct property slugs for an entity on one branch.
///
/// Forward-scans, seeking past every version of each property — reads roughly one
/// entry per property, not the full version history.
fn discover_props(
    branch: &BranchContext,
    entity: &Slug,
    on_branch: &Slug,
    props: &mut BTreeSet<Slug>,
) -> Result<(), DbError> {
    let entity_bound = AssertionKey::bound().with_prefix(|k| {
        k.branch = on_branch.clone();
        k.entity = entity.clone();
    });

    let mut iter = branch
        .storage()
        .scan_keys::<AssertionEntry>(&entity_bound)?;
    loop {
        let entry = match iter.next() {
            Some(Ok(e)) => e,
            Some(Err(e)) => return Err(e),
            None => break,
        };

        let prop = entry.prop.clone();
        props.insert(prop.clone());

        let skip = AssertionKey::bound().with_prefix(|k| {
            k.branch = on_branch.clone();
            k.entity = entity.clone();
            k.prop = prop.clone();
            k.tx_id = Uuid::max();
        });
        iter.seek(&skip);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::executor::checkout::ExecuteCheckout;
    use crate::command::executor::transaction::ExecuteTransaction;
    use crate::command::input::checkout::CheckoutInput;
    use crate::command::input::transaction::TransactionInput;
    use crate::command::Command;
    use crate::command::CommandState;
    use crate::config::{DataSchema, ProjectConfig};
    use crate::domain::entity::{Entity, PropertyValue as PV};
    use crate::domain::transaction::Transaction;
    use crate::domain::tx_meta::TxMeta;
    use crate::domain::validator::DomainValidator;
    use crate::store::storage::Storage;
    use indoc::indoc;
    use std::sync::Arc;

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
        "})
            .unwrap(),
        )
    }

    fn validator() -> DomainValidator {
        DomainValidator::new(test_schema())
    }

    fn meta(r: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut m = serde_json::Map::new();
        m.insert("reasoning".into(), serde_json::json!(r));
        m
    }

    fn exec(branch: &BranchContext, input: TransactionInput) -> Transaction<TxMeta> {
        let cmd = ExecuteTransaction::new(validator());
        let mut state = CommandState::new(branch.storage());
        let result = cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
        result
    }

    #[test]
    fn returns_latest_per_property() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(
            &branch,
            TransactionInput::new(meta("create")).write_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV {
                        supersedes_tx: None,
                        property: "age".parse().unwrap(),
                        value: serde_json::json!(25),
                        context: (),
                    },
                    PV {
                        supersedes_tx: None,
                        property: "city".parse().unwrap(),
                        value: serde_json::json!("London"),
                        context: (),
                    },
                ],
                meta("initial"),
            )),
        );

        exec(
            &branch,
            TransactionInput::new(meta("update")).write_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    supersedes_tx: None,
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(26),
                    context: (),
                }],
                meta("birthday"),
            )),
        );

        let props = properties(&branch, &"alice".parse().unwrap(), None).unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].key.prop.as_str(), "age");
        assert_eq!(props[0].value.value, serde_json::json!(26));
        assert_eq!(props[1].key.prop.as_str(), "city");
        assert_eq!(props[1].value.value, serde_json::json!("London"));
    }

    #[test]
    fn deleted_property_excluded() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(
            &branch,
            TransactionInput::new(meta("create")).write_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV {
                        supersedes_tx: None,
                        property: "age".parse().unwrap(),
                        value: serde_json::json!(25),
                        context: (),
                    },
                    PV {
                        supersedes_tx: None,
                        property: "city".parse().unwrap(),
                        value: serde_json::json!("London"),
                        context: (),
                    },
                ],
                meta("initial"),
            )),
        );

        exec(
            &branch,
            TransactionInput::new(meta("delete age")).write_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    supersedes_tx: None,
                    property: "age".parse().unwrap(),
                    value: serde_json::Value::Null,
                    context: (),
                }],
                meta("age retracted"),
            )),
        );

        let props = properties(&branch, &"alice".parse().unwrap(), None).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].key.prop.as_str(), "city");
        assert_eq!(props[0].value.value, serde_json::json!("London"));
    }

    #[test]
    fn at_tx_filters() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let tx1 = exec(
            &branch,
            TransactionInput::new(meta("create")).write_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![PV {
                    supersedes_tx: None,
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(25),
                    context: (),
                }],
                meta("initial"),
            )),
        );

        exec(
            &branch,
            TransactionInput::new(meta("update")).write_entity(Entity::new(
                "alice".parse().unwrap(),
                None,
                vec![PV {
                    supersedes_tx: None,
                    property: "age".parse().unwrap(),
                    value: serde_json::json!(26),
                    context: (),
                }],
                meta("birthday"),
            )),
        );

        let props =
            properties(&branch, &"alice".parse().unwrap(), Some(tx1.context.tx_id)).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value.value, serde_json::json!(25));
    }

    #[test]
    fn empty_for_unknown_entity() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let props = properties(&branch, &"ghost".parse().unwrap(), None).unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn inherits_from_parent_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();

        exec(
            &main,
            TransactionInput::new(meta("create")).write_entity(Entity::new(
                "alice".parse().unwrap(),
                Some(serde_json::json!("A person")),
                vec![
                    PV {
                        supersedes_tx: None,
                        property: "age".parse().unwrap(),
                        value: serde_json::json!(25),
                        context: (),
                    },
                    PV {
                        supersedes_tx: None,
                        property: "city".parse().unwrap(),
                        value: serde_json::json!("London"),
                        context: (),
                    },
                ],
                meta("initial"),
            )),
        );

        let checkout_cmd = ExecuteCheckout::new(validator());
        let mut state = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut state,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("update age")).write_entity(Entity::new(
                        "alice".parse().unwrap(),
                        None,
                        vec![PV {
                            supersedes_tx: None,
                            property: "age".parse().unwrap(),
                            value: serde_json::json!(30),
                            context: (),
                        }],
                        meta("changed on child"),
                    )),
                ),
            )
            .unwrap();
        state.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let props = properties(&child, &"alice".parse().unwrap(), None).unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].key.prop.as_str(), "age");
        assert_eq!(props[0].value.value, serde_json::json!(30));
        assert_eq!(props[0].key.branch.as_str(), "child");
        assert_eq!(props[1].key.prop.as_str(), "city");
        assert_eq!(props[1].value.value, serde_json::json!("London"));
        assert_eq!(props[1].key.branch.as_str(), "main");
    }

    // --- Status layering ---

    fn status_schema() -> Arc<DataSchema> {
        Arc::new(
            DataSchema::from_yaml(indoc! {"
            transaction_meta:
              reasoning: { type: text, required: true }
            entity_change_meta:
              reasoning: { type: text, required: true }
            branch_meta:
              reasoning: { type: text, required: true }
            assertion_statuses:
              values: [fact, hypothesis, observation]
              terminal: fact
              default: observation
        "})
            .unwrap(),
        )
    }

    fn exec_s(branch: &BranchContext, schema: Arc<DataSchema>, input: TransactionInput) {
        let cmd = ExecuteTransaction::new(DomainValidator::new(schema));
        let mut state = CommandState::new(branch.storage());
        cmd.execute(branch, &mut state, input).unwrap();
        state.commit().unwrap();
    }

    /// Build an entity asserting a single `capital` property with a status.
    fn capital(value: serde_json::Value, reasoning: &str, status: &str) -> Entity {
        Entity::new(
            "alice".parse().unwrap(),
            Some(serde_json::json!("a place")),
            vec![PV {
                supersedes_tx: None,
                property: "capital".parse().unwrap(),
                value,
                context: (),
            }],
            meta(reasoning),
        )
        .with_status(Some(status.into()))
    }

    #[test]
    fn bounded_selection_skips_large_sibling_and_descendant_ranges() {
        use crate::io::db_iterator::KEY_READS;
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let schema = status_schema();
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let slug: Slug = "alice".parse().unwrap();
        let mut paths = vec!["selected.title".to_owned()];
        for i in 0..300 {
            paths.push(format!("selected.group.p{i}.leaf"));
            paths.push(format!("unrelated.p{i}.leaf"));
        }
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("seed")).write_entity(
                Entity::new(
                    slug.clone(),
                    Some(serde_json::json!("test")),
                    paths
                        .iter()
                        .map(|p| PV {
                            property: p.parse().unwrap(),
                            value: serde_json::json!("payload"),
                            context: (),
                            supersedes_tx: None,
                        })
                        .collect(),
                    meta("seed"),
                )
                .with_status(Some("fact".into())),
            ),
        );
        KEY_READS.with(|n| n.set(0));
        let (values, refs) = selected_properties(
            &branch,
            &slug,
            None,
            Some(statuses),
            Some(&"selected".parse().unwrap()),
            Some(1),
        )
        .unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(
            refs.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
            ["selected.group"]
        );
        let selected_reads = KEY_READS.with(|n| n.get());
        assert_eq!(
            selected_reads, 2,
            "only title and one boundary key should be discovered"
        );
        KEY_READS.with(|n| n.set(0));
        let (values, refs) =
            selected_properties(&branch, &slug, None, Some(statuses), None, Some(1)).unwrap();
        assert!(values.is_empty());
        assert_eq!(refs.len(), 2);
        let root_reads = KEY_READS.with(|n| n.get());
        assert_eq!(root_reads, 2, "one discovered key per top-level subtree");
        KEY_READS.with(|n| n.set(0));
        assert_eq!(
            layered_properties(&branch, &slug, None, statuses)
                .unwrap()
                .len(),
            601
        );
        assert_eq!(KEY_READS.with(|n| n.get()), 601);
        eprintln!(
            "path discovery: full=601 keys, selected={selected_reads}, root depth1={root_reads}"
        );
    }

    #[test]
    fn selected_read_does_not_decode_omitted_values() {
        use crate::io::{storage_value::StorageValue, DbItem};
        struct InvalidValue;
        impl StorageValue for InvalidValue {
            fn encode(&self) -> Result<Vec<u8>, DbError> {
                Ok(b"not json".to_vec())
            }
            fn decode(_: &[u8]) -> Result<Self, DbError> {
                unreachable!()
            }
        }
        struct InvalidEntry(AssertionKey, InvalidValue);
        impl DbItem for InvalidEntry {
            type Key = AssertionKey;
            type Value = InvalidValue;
            fn cf() -> &'static str {
                AssertionEntry::cf()
            }
            fn key(&self) -> &AssertionKey {
                &self.0
            }
            fn value(&self) -> &InvalidValue {
                &self.1
            }
            fn from_parts(k: AssertionKey, v: InvalidValue) -> Self {
                Self(k, v)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let schema = status_schema();
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let slug: Slug = "alice".parse().unwrap();
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("seed")).write_entity(
                Entity::new(
                    slug.clone(),
                    Some(serde_json::json!("test")),
                    ["a", "a.deep.value", "other"]
                        .iter()
                        .map(|p| PV {
                            property: p.parse().unwrap(),
                            value: serde_json::json!(p),
                            context: (),
                            supersedes_tx: None,
                        })
                        .collect(),
                    meta("seed"),
                )
                .with_status(Some("fact".into())),
            ),
        );
        let all = layered_properties(&branch, &slug, None, statuses).unwrap();
        let mut batch = storage.batch();
        for a in &all {
            if a.key.prop.as_str() != "a" {
                batch
                    .put(&InvalidEntry(a.key.clone(), InvalidValue))
                    .unwrap();
            }
        }
        batch.commit().unwrap();
        let (values, refs) = selected_properties(
            &branch,
            &slug,
            None,
            Some(statuses),
            Some(&"a".parse().unwrap()),
            Some(0),
        )
        .unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].key.prop.as_str(), "a");
        assert_eq!(refs[0].as_str(), "a");
        assert!(layered_properties(&branch, &slug, None, statuses).is_err());
    }

    #[test]
    #[ignore = "manual performance probe; prints timings without flaky speed assertions"]
    fn benchmark_exact_conditions() {
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let schema = status_schema();
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let slug: Slug = "alice".parse().unwrap();
        let props = (0..200)
            .map(|n| PV {
                supersedes_tx: None,
                property: format!("p{n}").parse().unwrap(),
                value: serde_json::json!(n),
                context: (),
            })
            .collect();
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("seed")).write_entity(
                Entity::new(
                    slug.clone(),
                    Some(serde_json::json!("probe")),
                    props,
                    meta("seed"),
                )
                .with_status(Some("fact".into())),
            ),
        );
        for n in 0..200 {
            exec_s(
                &branch,
                schema.clone(),
                TransactionInput::new(meta("noise")).write_entity(capital(
                    serde_json::json!(n),
                    "unrelated history",
                    "observation",
                )),
            );
        }
        let keys: Vec<Slug> = (0..20).map(|n| format!("p{n}").parse().unwrap()).collect();
        for key in &keys {
            let full = layered_properties(&branch, &slug, None, statuses)
                .unwrap()
                .into_iter()
                .find(|a| &a.key.prop == key)
                .unwrap();
            let exact = exact_property(&branch, &slug, key, Some(statuses)).unwrap();
            assert_eq!(exact.len(), 1);
            assert_eq!(exact[0].value.value, full.value.value);
        }
        let start = Instant::now();
        for _ in 0..10 {
            for key in &keys {
                std::hint::black_box(
                    layered_properties(&branch, &slug, None, statuses)
                        .unwrap()
                        .into_iter()
                        .filter(|a| &a.key.prop == key)
                        .max_by_key(|a| a.key.tx_id),
                );
            }
        }
        let full = start.elapsed();
        let start = Instant::now();
        for _ in 0..10 {
            for key in &keys {
                std::hint::black_box(exact_property(&branch, &slug, key, Some(statuses)).unwrap());
            }
        }
        eprintln!("CAS probe: 200 properties + 200 unrelated overlays; 10 x 20 conditions; full={full:?}, exact={:?}",start.elapsed());
    }

    #[test]
    fn child_terminal_withdrawal_reveals_previous_baseline_without_resurrection() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        let schema = status_schema();
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let slug: Slug = "alice".parse().unwrap();
        for (text, status) in [
            ("buried", "observation"),
            ("base", "fact"),
            ("evidence", "observation"),
            ("summary", "fact"),
        ] {
            exec_s(
                &main,
                schema.clone(),
                TransactionInput::new(meta(text)).write_entity(capital(
                    serde_json::json!(text),
                    text,
                    status,
                )),
            );
        }
        let terminal = layered_properties(&main, &slug, None, statuses).unwrap()[0]
            .key
            .tx_id;
        let mut retract = capital(serde_json::Value::Null, "reopen", "fact");
        retract.properties[0].supersedes_tx = Some(terminal);
        let mut state = CommandState::new(&storage);
        ExecuteCheckout::new(DomainValidator::new(schema.clone()))
            .execute(
                &main,
                &mut state,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("fork"),
                    None,
                    TransactionInput::new(meta("withdraw inherited summary")).write_entity(retract),
                ),
            )
            .unwrap();
        state.commit().unwrap();
        let child = storage.branch("child".parse().unwrap()).unwrap();
        let current = layered_properties(&child, &slug, None, statuses).unwrap();
        let (selected, _) = selected_properties(
            &child,
            &slug,
            None,
            Some(statuses),
            Some(&"capital".parse().unwrap()),
            Some(0),
        )
        .unwrap();
        assert_eq!(
            selected.iter().map(|a| a.key.tx_id).collect::<Vec<_>>(),
            current.iter().map(|a| a.key.tx_id).collect::<Vec<_>>()
        );
        let (historical, _) = selected_properties(
            &child,
            &slug,
            Some(terminal),
            Some(statuses),
            Some(&"capital".parse().unwrap()),
            Some(0),
        )
        .unwrap();
        assert_eq!(historical[0].value.value, "summary");

        assert_eq!(
            current
                .iter()
                .map(|a| a.value.value.clone())
                .collect::<Vec<_>>(),
            vec![serde_json::json!("base"), serde_json::json!("evidence")]
        );
        assert_eq!(
            layered_properties(&main, &slug, None, statuses).unwrap()[0]
                .value
                .value,
            "summary"
        );
        assert_eq!(
            layered_properties(&child, &slug, Some(terminal), statuses).unwrap()[0]
                .value
                .value,
            "summary"
        );
    }

    #[test]
    fn replacing_terminal_reopens_evidence_for_next_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = storage.main_branch();
        let schema = status_schema();
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let slug: Slug = "alice".parse().unwrap();
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("old evidence")).write_entity(capital(
                serde_json::json!("old evidence"),
                "evidence",
                "observation",
            )),
        );
        let original = layered_properties(&branch, &slug, None, statuses).unwrap()[0]
            .key
            .tx_id;
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("summary")).write_entity(capital(
                serde_json::json!("summary"),
                "summary",
                "fact",
            )),
        );
        let terminal = layered_properties(&branch, &slug, None, statuses).unwrap()[0]
            .key
            .tx_id;
        let mut correction = capital(
            serde_json::json!("disputed summary"),
            "reopen",
            "observation",
        );
        correction.properties[0].supersedes_tx = Some(terminal);
        let mut invalid = correction.clone();
        let mut hidden = correction.properties[0].clone();
        hidden.supersedes_tx = Some(original);
        invalid.properties.push(hidden);
        let mut state = CommandState::new(branch.storage());
        assert!(
            ExecuteTransaction::new(DomainValidator::new(schema.clone()))
                .execute(
                    &branch,
                    &mut state,
                    TransactionInput::new(meta("cannot reveal and edit in same transaction"))
                        .write_entity(invalid)
                )
                .is_err()
        );
        assert_eq!(
            layered_properties(&branch, &slug, None, statuses).unwrap()[0]
                .key
                .tx_id,
            terminal
        );
        // The embedding preview must reveal the same material as the committed read.
        let staged = AssertionEntry {
            key: AssertionKey {
                branch: branch.id().clone(),
                entity: slug.clone(),
                prop: "capital".parse().unwrap(),
                tx_id: Uuid::now_v7(),
            },
            value: crate::store::entry::assertion::AssertionValue {
                supersedes_tx: Some(terminal),
                change_id: Uuid::nil(),
                value: correction.properties[0].value.clone(),
                reasoning: String::new(),
                status: Some("observation".into()),
                source: None,
            },
        };
        let preview = with_pending(&branch, &slug, Some(statuses), &[staged]).unwrap();
        assert_eq!(preview.len(), 2);
        assert!(preview.iter().any(|p| p.key.tx_id == original));
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("replace terminal")).write_entity(correction),
        );
        let current = layered_properties(&branch, &slug, None, statuses).unwrap();
        assert_eq!(current.len(), 2);
        assert_eq!(
            current[0].value.value,
            serde_json::json!("disputed summary")
        );
        assert!(current.iter().any(|p| p.key.tx_id == original));
        let mut withdrawal = capital(serde_json::Value::Null, "withdraw", "observation");
        withdrawal.properties[0].supersedes_tx = Some(current[0].key.tx_id);
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("withdraw correction")).write_entity(withdrawal),
        );
        let reopened = layered_properties(&branch, &slug, None, statuses).unwrap();
        let (selected, _) = selected_properties(
            &branch,
            &slug,
            None,
            Some(statuses),
            Some(&"capital".parse().unwrap()),
            Some(0),
        )
        .unwrap();
        assert_eq!(
            selected.iter().map(|a| a.key.tx_id).collect::<Vec<_>>(),
            reopened.iter().map(|a| a.key.tx_id).collect::<Vec<_>>()
        );

        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened[0].key.tx_id, original);
        let exact =
            exact_property(&branch, &slug, &"capital".parse().unwrap(), Some(statuses)).unwrap();
        assert_eq!(
            exact.iter().map(|p| p.key.tx_id).collect::<Vec<_>>(),
            reopened.iter().map(|p| p.key.tx_id).collect::<Vec<_>>()
        );
        let mut revoke_original =
            capital(serde_json::Value::Null, "withdraw revealed", "observation");
        revoke_original.properties[0].supersedes_tx = Some(original);
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("next request")).write_entity(revoke_original),
        );
        assert!(layered_properties(&branch, &slug, None, statuses)
            .unwrap()
            .is_empty());
        assert_eq!(
            layered_properties(&branch, &slug, Some(original), statuses).unwrap()[0]
                .value
                .value,
            serde_json::json!("old evidence")
        );
    }

    #[test]
    fn addressed_changes_preserve_siblings_history_and_reject_stale_targets() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        let schema = status_schema();
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let slug: Slug = "alice".parse().unwrap();
        let write = |branch: &BranchContext,
                     value: serde_json::Value,
                     status: &str,
                     target: Option<Uuid>| {
            let mut entity = capital(value, "test", status);
            entity.properties[0].supersedes_tx = target;
            let mut state = CommandState::new(branch.storage());
            let result = ExecuteTransaction::new(DomainValidator::new(schema.clone())).execute(
                branch,
                &mut state,
                TransactionInput::new(meta("test")).write_entity(entity),
            );
            match result {
                Ok(tx) => {
                    state.commit().unwrap();
                    Ok(tx.context.tx_id)
                }
                Err(e) => Err(e),
            }
        };
        let a = write(&main, serde_json::json!("a"), "observation", None).unwrap();
        let b = write(&main, serde_json::json!("b"), "observation", None).unwrap();
        // A terminal status on an addressed correction is not a global cutoff.
        let c = write(&main, serde_json::json!("c"), "fact", Some(a)).unwrap();
        let now = layered_properties(&main, &slug, None, statuses).unwrap();
        assert_eq!(
            now.iter().map(|p| p.key.tx_id).collect::<Vec<_>>(),
            vec![c, b]
        );
        assert_eq!(now[0].value.supersedes_tx, Some(a));
        // Latest-value consumers (especially CAS) must see C, not the older sibling B.
        let latest = properties(&main, &slug, None).unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].key.tx_id, c);
        let cas = |expected| {
            let mut state = CommandState::new(main.storage());
            ExecuteTransaction::new(DomainValidator::new(schema.clone())).execute(
                &main,
                &mut state,
                TransactionInput::new(meta("CAS")).require_property(
                    crate::command::input::transaction::PropertyPrecondition {
                        entity: slug.clone(),
                        property: "capital".parse().unwrap(),
                        expected: Some(serde_json::json!(expected)),
                    },
                ),
            )
        };
        assert!(cas("b").is_err());
        assert!(cas("c").is_ok());
        assert!(write(&main, serde_json::json!("stale"), "observation", Some(a)).is_err());
        assert!(write(
            &main,
            serde_json::json!("missing"),
            "observation",
            Some(Uuid::nil())
        )
        .is_err());
        let d = write(&main, serde_json::Value::Null, "fact", Some(c)).unwrap();
        let now = layered_properties(&main, &slug, None, statuses).unwrap();
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].key.tx_id, b);
        assert_eq!(
            layered_properties(&main, &slug, Some(c), statuses)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            layered_properties(&main, &slug, Some(a), statuses).unwrap()[0]
                .value
                .value,
            serde_json::json!("a")
        );
        assert!(write(
            &main,
            serde_json::json!("tombstone"),
            "observation",
            Some(d)
        )
        .is_err());
        // Fork sees inherited target; its retraction must not affect main.
        let mut state = CommandState::new(&storage);
        ExecuteCheckout::new(DomainValidator::new(schema.clone()))
            .execute(
                &main,
                &mut state,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("fork"),
                    None,
                    TransactionInput::new(meta("fork")),
                ),
            )
            .unwrap();
        state.commit().unwrap();
        let child = storage.branch("child".parse().unwrap()).unwrap();
        write(&child, serde_json::Value::Null, "fact", Some(b)).unwrap();
        assert!(layered_properties(&child, &slug, None, statuses)
            .unwrap()
            .is_empty());
        assert_eq!(
            layered_properties(&main, &slug, None, statuses)
                .unwrap()
                .len(),
            1
        );
        // A normal terminal write still consolidates all previous material.
        let summary = write(&main, serde_json::json!("summary"), "fact", None).unwrap();
        assert!(write(&main, serde_json::json!("hidden"), "observation", Some(b)).is_err());
        write(&main, serde_json::Value::Null, "fact", Some(summary)).unwrap();
        let reopened = layered_properties(&main, &slug, None, statuses).unwrap();
        assert_eq!(reopened.len(), 1);
        assert_eq!(reopened[0].key.tx_id, b);
        write(&main, serde_json::json!("x"), "observation", None).unwrap();
        let y = write(&main, serde_json::json!("y"), "observation", None).unwrap();
        write(&main, serde_json::Value::Null, "fact", Some(y)).unwrap();
        assert!(cas("x").is_ok());
        let mut state = CommandState::new(main.storage());
        assert!(
            ExecuteTransaction::new(DomainValidator::new(schema.clone()))
                .execute(
                    &main,
                    &mut state,
                    TransactionInput::new(meta("CAS absent")).require_property(
                        crate::command::input::transaction::PropertyPrecondition {
                            entity: slug.clone(),
                            property: "capital".parse().unwrap(),
                            expected: None
                        }
                    )
                )
                .is_err()
        );
    }

    #[test]
    fn multiple_terminals_consolidate_history_and_retract() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let mut schema = (*status_schema()).clone();
        let statuses = schema.assertion_statuses.as_mut().unwrap();
        statuses.values.push("summary".into());
        // First entry is the canonical status for deletion; neither has priority on reads.
        statuses.terminal = vec!["summary".into(), "fact".into()];
        let schema = Arc::new(schema);
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let slug: Slug = "alice".parse().unwrap();
        let mut historical = None;
        for (value, status) in [
            ("old", "fact"),
            ("guess", "hypothesis"),
            ("summary", "summary"),
            ("new observation", "observation"),
            ("new fact", "fact"),
        ] {
            exec_s(
                &branch,
                schema.clone(),
                TransactionInput::new(meta(value)).write_entity(capital(
                    serde_json::json!(value),
                    value,
                    status,
                )),
            );
            let props = layered_properties(&branch, &slug, None, statuses).unwrap();
            if value == "summary" {
                assert_eq!(props.len(), 1);
                historical = Some(props[0].key.tx_id);
            }
            if value == "new observation" {
                assert_eq!(props.len(), 2);
                assert_eq!(props[0].value.value, serde_json::json!("summary"));
            }
            if value == "new fact" {
                assert_eq!(props.len(), 1);
                assert_eq!(props[0].value.value, serde_json::json!("new fact"));
            }
        }
        let old = layered_properties(&branch, &slug, historical, statuses).unwrap();
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].value.value, serde_json::json!("summary"));
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("delete")).delete_entity(
                crate::command::input::transaction::DeleteItem::new(
                    slug.clone(),
                    serde_json::json!("test"),
                ),
            ),
        );
        assert!(layered_properties(&branch, &slug, None, statuses)
            .unwrap()
            .is_empty());
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("recreate")).write_entity(capital(
                serde_json::json!("fresh"),
                "recreate",
                "observation",
            )),
        );
        let props = layered_properties(&branch, &slug, None, statuses).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value.value, serde_json::json!("fresh"));
    }

    #[test]
    fn layering_latest_fact_plus_later_overlays() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let schema = status_schema();

        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t1")).write_entity(capital(
                serde_json::json!("Lyon"),
                "old fact",
                "fact",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t2")).write_entity(capital(
                serde_json::json!("Paris?"),
                "guess",
                "hypothesis",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t3")).write_entity(capital(
                serde_json::json!("doc says Paris"),
                "obs",
                "observation",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t4")).write_entity(capital(
                serde_json::json!("Paris"),
                "settled",
                "fact",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t5")).write_entity(capital(
                serde_json::json!("might move"),
                "new guess",
                "hypothesis",
            )),
        );

        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();

        // Baseline fact@t4 + the single hypothesis thrown after it (t5).
        // Everything before t4 is consolidated away.
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].value.value, serde_json::json!("Paris"));
        assert_eq!(props[0].value.status.as_deref(), Some("fact"));
        assert_eq!(props[1].value.value, serde_json::json!("might move"));
        assert_eq!(props[1].value.status.as_deref(), Some("hypothesis"));
    }

    #[test]
    fn layering_no_fact_returns_all_overlays() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let schema = status_schema();

        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t1")).write_entity(capital(
                serde_json::json!("Paris?"),
                "guess",
                "hypothesis",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t2")).write_entity(capital(
                serde_json::json!("seen Paris"),
                "obs",
                "observation",
            )),
        );

        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();
        assert_eq!(props.len(), 2);
        assert!(props
            .iter()
            .all(|p| p.value.status.as_deref() != Some("fact")));
    }

    #[test]
    fn layering_drops_overlays_older_than_fact() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let schema = status_schema();

        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t1")).write_entity(capital(
                serde_json::json!("Paris?"),
                "guess",
                "hypothesis",
            )),
        );
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t2")).write_entity(capital(
                serde_json::json!("Paris"),
                "settled",
                "fact",
            )),
        );

        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();
        // The earlier hypothesis is consolidated by the fact — only the fact remains.
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value.value, serde_json::json!("Paris"));
        assert_eq!(props[0].value.status.as_deref(), Some("fact"));
    }

    #[test]
    fn layering_retraction_suppresses_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let schema = status_schema();

        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t1")).write_entity(capital(
                serde_json::json!("Paris"),
                "settled",
                "fact",
            )),
        );
        // Retract via a null-valued fact.
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t2")).write_entity(capital(
                serde_json::Value::Null,
                "retract",
                "fact",
            )),
        );

        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();
        assert!(props.is_empty());

        // A later hypothesis re-opens the property on top of the retraction.
        exec_s(
            &branch,
            schema.clone(),
            TransactionInput::new(meta("t3")).write_entity(capital(
                serde_json::json!("maybe Lyon"),
                "reopen",
                "hypothesis",
            )),
        );
        let props = layered_properties(&branch, &"alice".parse().unwrap(), None, statuses).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value.status.as_deref(), Some("hypothesis"));
    }

    #[test]
    fn layering_across_ancestry() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = storage.main_branch();
        let mut schema = (*status_schema()).clone();
        schema
            .assertion_statuses
            .as_mut()
            .unwrap()
            .values
            .push("summary".into());
        schema
            .assertion_statuses
            .as_mut()
            .unwrap()
            .terminal
            .push("summary".into());
        let schema = Arc::new(schema);

        exec_s(
            &main,
            schema.clone(),
            TransactionInput::new(meta("t1")).write_entity(capital(
                serde_json::json!("Paris"),
                "settled",
                "fact",
            )),
        );

        let checkout_cmd = ExecuteCheckout::new(DomainValidator::new(schema.clone()));
        let mut state = CommandState::new(&storage);
        checkout_cmd
            .execute(
                &main,
                &mut state,
                CheckoutInput::new(
                    "child".parse().unwrap(),
                    meta("explore"),
                    None,
                    TransactionInput::new(meta("guess on child")).write_entity(capital(
                        serde_json::json!("maybe Lyon"),
                        "child guess",
                        "hypothesis",
                    )),
                ),
            )
            .unwrap();
        state.commit().unwrap();

        let child = storage.branch("child".parse().unwrap()).unwrap();
        let statuses = schema.assertion_statuses.as_ref().unwrap();
        let props = layered_properties(&child, &"alice".parse().unwrap(), None, statuses).unwrap();
        // Baseline fact inherited from main + hypothesis thrown on the child.
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].value.value, serde_json::json!("Paris"));
        assert_eq!(props[0].value.status.as_deref(), Some("fact"));
        assert_eq!(props[0].key.branch.as_str(), "main");
        assert_eq!(props[1].value.status.as_deref(), Some("hypothesis"));
        assert_eq!(props[1].key.branch.as_str(), "child");
        exec_s(
            &child,
            schema.clone(),
            TransactionInput::new(meta("consolidate child")).write_entity(capital(
                serde_json::json!("child summary"),
                "summary",
                "summary",
            )),
        );
        let props = layered_properties(&child, &"alice".parse().unwrap(), None, statuses).unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].value.value, serde_json::json!("child summary"));
        let parent = layered_properties(&main, &"alice".parse().unwrap(), None, statuses).unwrap();
        assert_eq!(parent.len(), 1);
        assert_eq!(parent[0].value.value, serde_json::json!("Paris"));
    }

    #[test]
    fn sorted_by_slug() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        exec(
            &branch,
            TransactionInput::new(meta("create")).write_entity(Entity::new(
                "server".parse().unwrap(),
                Some(serde_json::json!("A server")),
                vec![
                    PV {
                        supersedes_tx: None,
                        property: "zone".parse().unwrap(),
                        value: serde_json::json!("us-east"),
                        context: (),
                    },
                    PV {
                        supersedes_tx: None,
                        property: "cpu".parse().unwrap(),
                        value: serde_json::json!(8),
                        context: (),
                    },
                    PV {
                        supersedes_tx: None,
                        property: "memory".parse().unwrap(),
                        value: serde_json::json!("32gb"),
                        context: (),
                    },
                ],
                meta("initial"),
            )),
        );

        let props = properties(&branch, &"server".parse().unwrap(), None).unwrap();
        let slugs: Vec<&str> = props.iter().map(|p| p.key.prop.as_str()).collect();
        assert_eq!(slugs, vec!["cpu", "memory", "zone"]);
    }
}
