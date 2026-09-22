use super::*;
fn section(parent: Option<Uuid>, title: &str) -> Content {
    Content {
        parent,
        position: 0,
        kind: Kind::Section,
        title: Some(title.into()),
        body: String::new(),
        state: None,
        deleted: false,
    }
}
fn edit(id: Uuid, expected: Option<Uuid>, content: Content) -> Edit {
    Edit {
        id,
        expected,
        content,
    }
}
#[test]
fn move_keeps_descendants_and_historical_tree_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let child = Uuid::now_v7();
    let leaf = Uuid::now_v7();
    let t1 = store
        .apply(
            "test",
            "seed",
            vec![
                edit(a, None, section(None, "A")),
                edit(b, None, section(None, "B")),
                edit(child, None, section(Some(a), "child")),
                edit(leaf, None, section(Some(child), "leaf")),
            ],
        )
        .unwrap();
    let t2 = store
        .apply(
            "test",
            "move",
            vec![edit(child, Some(t1), section(Some(b), "renamed"))],
        )
        .unwrap();
    assert_eq!(store.changed_blocks(t2).unwrap(), vec![child]);
    assert_eq!(store.get(leaf, None).unwrap().unwrap().tx, t1);
    assert!(store.children(a, None).unwrap().is_empty());
    assert_eq!(store.children(b, None).unwrap()[0].id, child);
    assert_eq!(
        store.children(a, Some(t1)).unwrap()[0]
            .content
            .title
            .as_deref(),
        Some("child")
    );
    assert!(store.children(b, Some(t1)).unwrap().is_empty());
    assert_eq!(store.children(child, Some(t2)).unwrap()[0].id, leaf);
    assert_eq!(store.history(child, None, 10).unwrap().len(), 2);
    assert_eq!(store.transaction(t2).unwrap().unwrap().reason, "move");
    drop(store);
    let store = DocumentStore::open(dir.path()).unwrap();
    assert_eq!(store.children(a, Some(t1)).unwrap()[0].id, child);
    assert_eq!(store.children(b, None).unwrap()[0].id, child);
}
#[test]
fn invalid_batch_is_atomic_and_final_topology_is_validated() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let t = store
        .apply(
            "test",
            "seed",
            vec![
                edit(a, None, section(None, "A")),
                edit(b, None, section(Some(a), "B")),
            ],
        )
        .unwrap();
    assert!(store
        .apply(
            "test",
            "cycle",
            vec![edit(a, Some(t), section(Some(b), "A"))]
        )
        .is_err());
    let mut deleted = section(None, "A");
    deleted.deleted = true;
    assert!(store
        .apply(
            "test",
            "delete parent",
            vec![edit(a, Some(t), deleted.clone())]
        )
        .is_err());
    assert!(store
        .apply(
            "test",
            "stale",
            vec![
                edit(a, Some(t), section(None, "changed")),
                edit(b, None, section(None, "B"))
            ]
        )
        .is_err());
    assert_eq!(
        store
            .get(a, None)
            .unwrap()
            .unwrap()
            .content
            .title
            .as_deref(),
        Some("A")
    );
    let t2 = store
        .apply(
            "test",
            "move child out and delete parent",
            vec![
                edit(a, Some(t), deleted),
                edit(b, Some(t), section(None, "B")),
            ],
        )
        .unwrap();
    assert!(store.get(a, None).unwrap().unwrap().content.deleted);
    assert_eq!(store.children(a, Some(t)).unwrap().len(), 1);
    assert!(store.children(a, Some(t2)).unwrap().is_empty());
    assert!(store
        .apply(
            "test",
            "revive without revision",
            vec![edit(a, None, section(None, "A"))]
        )
        .is_err());
    store
        .apply(
            "test",
            "restore",
            vec![edit(a, Some(t2), section(None, "A"))],
        )
        .unwrap();
}
#[test]
fn sibling_order_and_membership_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let mut first = section(Some(root), "A");
    first.position = 10;
    let mut second = section(Some(root), "B");
    second.position = 20;
    let t1 = store
        .apply(
            "test",
            "seed",
            vec![
                edit(root, None, section(None, "root")),
                edit(a, None, first),
                edit(b, None, second.clone()),
            ],
        )
        .unwrap();
    second.position = 0;
    store
        .apply("test", "reorder", vec![edit(b, Some(t1), second)])
        .unwrap();
    assert_eq!(
        store
            .children(root, None)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![b, a]
    );
    assert_eq!(
        store
            .children(root, Some(t1))
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![a, b]
    );
}
#[test]
fn competing_writers_cannot_both_replace_same_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let id = Uuid::now_v7();
    let tx = store
        .apply("test", "seed", vec![edit(id, None, section(None, "A"))])
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let mut workers = vec![];
    for _ in 0..2 {
        let s = store.clone();
        let b = barrier.clone();
        workers.push(std::thread::spawn(move || {
            b.wait();
            s.apply("test", "edit", vec![edit(id, Some(tx), section(None, "B"))])
                .is_ok()
        }));
    }
    barrier.wait();
    assert_eq!(
        workers
            .into_iter()
            .map(|w| usize::from(w.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert_eq!(store.history(id, None, 10).unwrap().len(), 2);
}

#[test]
fn subtree_depth_is_bounded_and_budget_is_not_silent_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let child = Uuid::now_v7();
    let leaf = Uuid::now_v7();
    store
        .apply(
            "test",
            "tree",
            vec![
                edit(root, None, section(None, "root")),
                edit(child, None, section(Some(root), "child")),
                edit(leaf, None, section(Some(child), "leaf")),
            ],
        )
        .unwrap();
    assert_eq!(store.subtree(root, None, 0, 1).unwrap().len(), 1);
    assert_eq!(store.subtree(root, None, 1, 2).unwrap().len(), 2);
    assert!(store.subtree(root, None, 2, 2).is_err());
    assert_eq!(store.subtree(root, None, 2, 3).unwrap().len(), 3);
}

#[test]
fn historical_children_seek_past_long_histories() {
    use crate::io::db_iterator::{KEY_READS, VALUE_READS};
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let child = Uuid::now_v7();
    let removed = Uuid::now_v7();
    let future = Uuid::from_u128(u128::MAX - 1);
    let mut tx = store
        .apply(
            "test",
            "seed",
            vec![
                edit(root, None, section(None, "root")),
                edit(child, None, section(Some(root), "child")),
                edit(removed, None, section(Some(root), "removed")),
            ],
        )
        .unwrap();
    let first = tx;
    let mut middle = tx;
    for position in 1..=128 {
        let mut content = section(Some(root), "child");
        content.position = position;
        tx = store
            .apply("test", "reorder", vec![edit(child, Some(tx), content)])
            .unwrap();
        if position == 64 {
            middle = tx;
        }
    }
    let mut tombstone = section(Some(root), "removed");
    tombstone.deleted = true;
    let deletion = store
        .apply(
            "test",
            "remove",
            vec![edit(removed, Some(first), tombstone)],
        )
        .unwrap();
    store
        .apply(
            "test",
            "later child",
            vec![edit(future, None, section(Some(root), "future"))],
        )
        .unwrap();
    for at in [first, middle, deletion] {
        // Deliberately slow reference implementation, independent of the seek loop.
        let mut oracle = BTreeMap::new();
        for entry in store
            .inner
            .db
            .scan::<ChildVersion>(&ChildVersionKey::bound().with_prefix(|k| k.parent = root))
            .unwrap()
        {
            let entry = entry.unwrap();
            if entry.key.tx <= at {
                oracle.insert(entry.key.child, entry.value.present);
            }
        }
        let expected: HashSet<_> = oracle
            .into_iter()
            .filter(|(_, present)| *present)
            .map(|(id, _)| id)
            .collect();
        KEY_READS.with(|n| n.set(0));
        VALUE_READS.with(|n| n.set(0));
        let actual = store.children(root, Some(at)).unwrap();
        let keys = KEY_READS.with(|n| n.get());
        let values = VALUE_READS.with(|n| n.get());
        assert_eq!(
            actual.iter().map(|b| b.id).collect::<HashSet<_>>(),
            expected
        );
        assert_eq!(keys, 3, "one key per ever-associated child");
        assert!(
            values <= 6,
            "decoded {values} records instead of bounded seeks"
        );
        if at == middle {
            assert_eq!(
                actual
                    .iter()
                    .find(|b| b.id == child)
                    .unwrap()
                    .content
                    .position,
                64
            );
        }
    }
    assert!(store.children(root, Some(Uuid::nil())).unwrap().is_empty());
}

#[test]
fn lists_have_containers_nested_items_and_checkbox_history() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let list = Uuid::now_v7();
    let item = Uuid::now_v7();
    let nested = Uuid::now_v7();
    let text = Uuid::now_v7();
    let mut l = section(Some(root), "list");
    l.kind = Kind::List {
        start: Some(3),
        spread: false,
    };
    l.title = None;
    let mut i = section(Some(list), "item");
    i.kind = Kind::ListItem { spread: false };
    i.title = None;
    i.state = Some(TaskState::Todo);
    let mut n = section(Some(item), "nested");
    n.kind = Kind::List {
        start: None,
        spread: false,
    };
    n.title = None;
    let mut t = section(Some(item), "paragraphs");
    t.kind = Kind::Text;
    t.title = None;
    t.body = "First paragraph.\n\nSecond paragraph.".into();
    let tx = store
        .apply(
            "test",
            "nested list",
            vec![
                edit(root, None, section(None, "root")),
                edit(list, None, l.clone()),
                edit(item, None, i.clone()),
                edit(nested, None, n),
                edit(text, None, t),
            ],
        )
        .unwrap();
    assert_eq!(store.subtree(root, None, 3, 5).unwrap().len(), 5);
    i.state = Some(TaskState::Done);
    store
        .apply("test", "done", vec![edit(item, Some(tx), i)])
        .unwrap();
    assert_eq!(
        store.get(item, Some(tx)).unwrap().unwrap().content.state,
        Some(TaskState::Todo)
    );
    let mut bad = section(Some(list), "bad");
    bad.kind = Kind::Text;
    bad.title = None;
    assert!(store
        .apply(
            "test",
            "invalid list child",
            vec![edit(Uuid::now_v7(), None, bad)]
        )
        .is_err());
    l.kind = Kind::Text;
    l.title = None;
    assert!(store
        .apply(
            "test",
            "invalid container change",
            vec![edit(list, Some(tx), l)]
        )
        .is_err());
}

#[test]
fn relative_placement_rebalances_atomically_and_rejects_stale_anchor() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let c = Uuid::now_v7();
    let mut second = section(Some(root), "B");
    second.position = 1;
    let original = store
        .apply(
            "test",
            "adjacent ranks",
            vec![
                edit(root, None, section(None, "root")),
                edit(a, None, section(Some(root), "A")),
                edit(b, None, second),
            ],
        )
        .unwrap();
    let inserted = store
        .place(
            "test",
            "insert between",
            edit(c, None, section(None, "C")),
            Placement::Before {
                anchor: b,
                expected: Some(original),
            },
        )
        .unwrap();
    assert_eq!(
        store
            .children(root, None)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![a, c, b]
    );
    assert_eq!(store.changed_blocks(inserted).unwrap().len(), 3);
    assert_eq!(
        store
            .children(root, Some(original))
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![a, b]
    );
    assert!(store
        .place(
            "test",
            "stale",
            edit(Uuid::now_v7(), None, section(None, "D")),
            Placement::After {
                anchor: b,
                expected: Some(original)
            }
        )
        .is_err());
    let moved = store
        .place(
            "test",
            "move to end",
            edit(a, Some(inserted), section(None, "A")),
            Placement::Last { parent: root },
        )
        .unwrap();
    assert_eq!(
        store
            .children(root, None)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![c, b, a]
    );
    assert_eq!(store.changed_blocks(moved).unwrap(), vec![a]);
    assert!(store
        .place(
            "test",
            "self",
            edit(a, Some(moved), section(None, "A")),
            Placement::Before {
                anchor: a,
                expected: Some(moved)
            }
        )
        .is_err());
}

#[test]
fn subtree_budget_stops_before_loading_wide_children() {
    use crate::io::db_iterator::{KEY_READS, VALUE_READS};
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let mut edits = vec![edit(root, None, section(None, "root"))];
    for _ in 0..100 {
        let mut c = section(Some(root), "child");
        c.kind = Kind::Text;
        c.title = None;
        c.body = "large body ".repeat(1000);
        edits.push(edit(Uuid::now_v7(), None, c));
    }
    let tx = store.apply("test", "wide tree", edits).unwrap();
    for at in [None, Some(tx)] {
        KEY_READS.with(|n| n.set(0));
        VALUE_READS.with(|n| n.set(0));
        assert!(store.subtree(root, at, 1, 1).is_err());
        assert_eq!(KEY_READS.with(|n| n.get()), 1);
        assert!(VALUE_READS.with(|n| n.get()) <= 3);
    }
}

fn text(parent: Uuid, body: &str) -> Content {
    Content {
        parent: Some(parent),
        position: 0,
        kind: Kind::Text,
        title: None,
        body: body.into(),
        state: None,
        deleted: false,
    }
}
#[test]
fn compound_transaction_reads_its_own_placements_and_commits_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let list = Uuid::now_v7();
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let txt = Uuid::now_v7();
    let mut l = section(Some(root), "");
    l.title = None;
    l.kind = Kind::List {
        start: Some(1),
        spread: false,
    };
    let mut item = section(Some(list), "");
    item.title = None;
    item.kind = Kind::ListItem { spread: false };
    let tx = store
        .transact(
            "test",
            "create and arrange a list",
            vec![
                Operation::Put(edit(root, None, section(None, "root"))),
                Operation::Place {
                    edit: edit(list, None, l),
                    placement: Placement::Last { parent: root },
                },
                Operation::Place {
                    edit: edit(a, None, item.clone()),
                    placement: Placement::Last { parent: list },
                },
                Operation::Place {
                    edit: edit(b, None, item),
                    placement: Placement::Before {
                        anchor: a,
                        expected: None,
                    },
                },
                Operation::Put(edit(
                    txt,
                    None,
                    text(b, "Two paragraphs.\n\nStill one block."),
                )),
                Operation::Put(edit(root, None, section(None, "renamed"))),
            ],
        )
        .unwrap();
    assert_eq!(
        store
            .children(list, None)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![b, a]
    );
    assert_eq!(store.changed_blocks(tx).unwrap().len(), 5);
    assert_eq!(store.history(root, None, 10).unwrap().len(), 1);
    assert_eq!(
        store
            .get(root, None)
            .unwrap()
            .unwrap()
            .content
            .title
            .as_deref(),
        Some("renamed")
    );
    let failed = Uuid::now_v7();
    let result = store.transact(
        "test",
        "must roll back",
        vec![
            Operation::Place {
                edit: edit(failed, None, text(root, "unsaved")),
                placement: Placement::Last { parent: root },
            },
            Operation::Put(edit(root, None, section(None, "stale"))),
        ],
    );
    assert!(matches!(result, Err(DocumentError::Conflict(_))));
    assert!(store.get(failed, None).unwrap().is_none());
    assert_eq!(store.children(root, None).unwrap().len(), 1);
}

#[test]
fn common_placement_remains_cheap_beyond_one_thousand_siblings() {
    use crate::io::db_iterator::{KEY_READS, VALUE_READS};
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    store
        .apply(
            "test",
            "root",
            vec![edit(root, None, section(None, "root"))],
        )
        .unwrap();
    for batch in 0..2 {
        let edits = (0..600)
            .map(|i| {
                let mut c = text(root, "heavy ".repeat(100).as_str());
                c.position = (batch * 600 + i) * 1024;
                edit(Uuid::now_v7(), None, c)
            })
            .collect();
        store.apply("test", "wide section", edits).unwrap();
    }
    KEY_READS.with(|n| n.set(0));
    VALUE_READS.with(|n| n.set(0));
    let id = Uuid::now_v7();
    store
        .place(
            "test",
            "append",
            edit(id, None, text(root, "last")),
            Placement::Last { parent: root },
        )
        .unwrap();
    assert!(
        VALUE_READS.with(|n| n.get()) < 12,
        "append must not read sibling bodies"
    );
    assert!(KEY_READS.with(|n| n.get()) < 5);
    assert_eq!(store.children(root, None).unwrap().len(), 1201);
    assert_eq!(store.children(root, None).unwrap().last().unwrap().id, id);
}

#[test]
fn canonical_payloads_and_error_categories_are_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let mut bad = section(None, "root");
    bad.body = "ambiguous container text".into();
    assert!(matches!(
        store.apply("test", "bad", vec![edit(root, None, bad)]),
        Err(DocumentError::InvalidInput(_))
    ));
    let tx = store
        .apply(
            "test",
            "root",
            vec![edit(root, None, section(None, "root"))],
        )
        .unwrap();
    let mut bad = text(root, "text");
    bad.title = Some("ambiguous title".into());
    assert!(matches!(
        store.apply("test", "bad", vec![edit(Uuid::now_v7(), None, bad)]),
        Err(DocumentError::InvalidInput(_))
    ));
    assert!(matches!(
        store.history(root, None, 1001),
        Err(DocumentError::Limit(_))
    ));
    assert!(matches!(
        store.place(
            "test",
            "bad anchor",
            edit(Uuid::now_v7(), None, text(root, "x")),
            Placement::After {
                anchor: root,
                expected: None
            }
        ),
        Err(DocumentError::Conflict(_))
    ));
    assert_eq!(store.get(root, None).unwrap().unwrap().tx, tx);
}

#[test]
fn staged_moves_hide_old_index_positions() {
    let dir = tempfile::tempdir().unwrap();
    let store = DocumentStore::open(dir.path()).unwrap();
    let root = Uuid::now_v7();
    let other = Uuid::now_v7();
    let a = Uuid::now_v7();
    let b = Uuid::now_v7();
    let c = Uuid::now_v7();
    let mut ac = text(root, "A");
    ac.position = 1024;
    let mut bc = text(root, "B");
    bc.position = 2048;
    let tx = store
        .apply(
            "test",
            "seed",
            vec![
                edit(root, None, section(None, "root")),
                edit(other, None, section(None, "other")),
                edit(a, None, ac),
                edit(b, None, bc),
            ],
        )
        .unwrap();
    let after = store
        .transact(
            "test",
            "move then insert",
            vec![
                Operation::Place {
                    edit: edit(a, Some(tx), text(other, "A")),
                    placement: Placement::Last { parent: other },
                },
                Operation::Place {
                    edit: edit(b, Some(tx), text(other, "B")),
                    placement: Placement::Before {
                        anchor: a,
                        expected: Some(tx),
                    },
                },
                Operation::Place {
                    edit: edit(c, None, text(root, "C")),
                    placement: Placement::Last { parent: root },
                },
            ],
        )
        .unwrap();
    assert_eq!(
        store
            .children(root, None)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![c]
    );
    assert_eq!(
        store
            .children(other, None)
            .unwrap()
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        vec![b, a]
    );
    assert_eq!(store.children(root, Some(tx)).unwrap().len(), 2);
    assert_eq!(store.changed_blocks(after).unwrap().len(), 3);
}
