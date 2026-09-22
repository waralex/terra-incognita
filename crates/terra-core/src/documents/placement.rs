use super::*;

/// Anchor revision is its committed revision before this transaction.
/// None refers to an anchor created by an earlier operation in this transaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Placement {
    First {
        parent: Uuid,
    },
    Last {
        parent: Uuid,
    },
    Before {
        anchor: Uuid,
        expected: Option<Uuid>,
    },
    After {
        anchor: Uuid,
        expected: Option<Uuid>,
    },
}
#[derive(Serialize, Deserialize)]
pub enum Operation {
    Put(Edit),
    Place { edit: Edit, placement: Placement },
}
struct Planner<'a> {
    store: &'a DocumentStore,
    pending: BTreeMap<Uuid, Edit>,
}
impl Planner<'_> {
    fn content(&self, id: Uuid) -> Result<Option<(Content, Option<Uuid>)>, DocumentError> {
        if let Some(e) = self.pending.get(&id) {
            return Ok(Some((e.content.clone(), e.expected)));
        }
        Ok(self.store.head(id)?.map(|b| (b.content, Some(b.tx))))
    }
    fn stage(&mut self, edit: Edit) -> Result<(), DocumentError> {
        if edit.id == Uuid::nil() || edit.id == Uuid::max() || edit.content.position < 0 {
            return Err(invalid("invalid block ID or negative position"));
        }
        let revision = match self.pending.get(&edit.id) {
            Some(e) => e.expected,
            None => self.store.head(edit.id)?.map(|b| b.tx),
        };
        if revision != edit.expected {
            return Err(conflict("stale block revision"));
        }
        self.pending.insert(edit.id, edit);
        if self.pending.len() > 1000 {
            return Err(exceeded("transaction exceeds 1000 changed blocks"));
        }
        Ok(())
    }
    // A directional neighbor seek over current keys plus the transaction overlay.
    // Staged keys hide their old committed positions. No sibling bodies are read.
    fn neighbor(
        &self,
        parent: Uuid,
        pivot: Option<(i64, Uuid)>,
        reverse: bool,
        exclude: Uuid,
    ) -> Result<Option<(i64, Uuid)>, DocumentError> {
        let eligible = |key: &(i64, Uuid)| {
            key.1 != exclude && pivot.is_none_or(|p| if reverse { *key < p } else { *key > p })
        };
        let mut bound = ChildKey::bound()
            .with_prefix(|k| k.parent = parent)
            .with_lower(|k| k.position = 0);
        if let Some((position, child)) = pivot {
            if reverse {
                bound = bound.with_upper(|k| {
                    k.position = position;
                    k.child = child;
                });
            } else {
                bound = bound.with_lower(|k| {
                    k.position = position;
                    k.child = child;
                });
            }
        }
        let mut iter = if reverse {
            self.store.inner.db.scan_rev::<Child>(&bound)?
        } else {
            self.store.inner.db.scan::<Child>(&bound)?
        };
        let mut candidate = None;
        for entry in &mut iter {
            let key = entry?.key;
            let pair = (key.position, key.child);
            if eligible(&pair) && !self.pending.contains_key(&key.child) {
                candidate = Some(pair);
                break;
            }
        }
        for (&id, e) in &self.pending {
            let pair = (e.content.position, id);
            if !e.content.deleted
                && e.content.parent == Some(parent)
                && eligible(&pair)
                && candidate.is_none_or(|old| if reverse { pair > old } else { pair < old })
            {
                candidate = Some(pair);
            }
        }
        Ok(candidate)
    }
    fn siblings(&self, parent: Uuid, exclude: Uuid) -> Result<Vec<(i64, Uuid)>, DocumentError> {
        let mut keys = Vec::new();
        for key in self.store.inner.db.scan_keys::<Child>(
            &ChildKey::bound()
                .with_prefix(|k| k.parent = parent)
                .with_lower(|k| k.position = 0),
        )? {
            let key = key?;
            if key.child != exclude && !self.pending.contains_key(&key.child) {
                keys.push((key.position, key.child));
            }
            if keys.len() >= 1000 {
                return Err(exceeded("rebalance exceeds 1000 changed blocks"));
            }
        }
        for (&id, e) in &self.pending {
            if id != exclude && !e.content.deleted && e.content.parent == Some(parent) {
                keys.push((e.content.position, id));
            }
        }
        if keys.len() >= 1000 {
            return Err(exceeded("rebalance exceeds 1000 changed blocks"));
        }
        keys.sort();
        Ok(keys)
    }
    fn place(&mut self, mut edit: Edit, placement: Placement) -> Result<(), DocumentError> {
        if edit.content.deleted {
            return Err(invalid("cannot place a deleted block"));
        }
        let (parent, anchor) = match placement {
            Placement::First { parent } | Placement::Last { parent } => (parent, None),
            Placement::Before { anchor, expected } | Placement::After { anchor, expected } => {
                if anchor == edit.id {
                    return Err(invalid("cannot place relative to self"));
                }
                let (content, revision) = self
                    .content(anchor)?
                    .ok_or_else(|| invalid("missing anchor"))?;
                if revision != expected || content.deleted {
                    return Err(conflict("stale anchor revision"));
                }
                (
                    content
                        .parent
                        .ok_or_else(|| invalid("root has no sibling placement"))?,
                    Some((content.position, anchor)),
                )
            }
        };
        let (left, right) = match placement {
            Placement::First { .. } => (None, self.neighbor(parent, None, false, edit.id)?),
            Placement::Last { .. } => (self.neighbor(parent, None, true, edit.id)?, None),
            Placement::Before { .. } => (self.neighbor(parent, anchor, true, edit.id)?, anchor),
            Placement::After { .. } => (anchor, self.neighbor(parent, anchor, false, edit.id)?),
        };
        let rank = match (left.map(|p| p.0), right.map(|p| p.0)) {
            (None, None) => Some(1024),
            (None, Some(r)) if r > 0 => Some(r / 2),
            (Some(l), None) => l.checked_add(1024),
            (Some(l), Some(r)) if r - l > 1 => Some(l + (r - l) / 2),
            _ => None,
        };
        edit.content.parent = Some(parent);
        if let Some(rank) = rank {
            edit.content.position = rank;
        } else {
            let siblings = self.siblings(parent, edit.id)?;
            let index = match placement {
                Placement::First { .. } => 0,
                Placement::Last { .. } => siblings.len(),
                Placement::Before { .. } | Placement::After { .. } => {
                    siblings
                        .iter()
                        .position(|p| Some(*p) == anchor)
                        .ok_or_else(|| invalid("missing anchor in siblings"))?
                        + usize::from(matches!(placement, Placement::After { .. }))
                }
            };
            for (i, (old, id)) in siblings.into_iter().enumerate() {
                let rank = ((i + usize::from(i >= index) + 1) as i64) * 1024;
                if old != rank {
                    let (mut content, expected) = self
                        .content(id)?
                        .ok_or_else(|| invalid("missing sibling"))?;
                    content.position = rank;
                    self.stage(Edit {
                        id,
                        expected,
                        content,
                    })?;
                }
            }
            edit.content.position = ((index + 1) as i64) * 1024;
        }
        self.stage(edit)
    }
}
impl DocumentStore {
    /// Plan sequential intents against a shared overlay, validate final topology,
    /// and commit exactly once. Expected revisions always refer to committed heads.
    pub fn transact(
        &self,
        author: &str,
        reason: &str,
        operations: Vec<Operation>,
    ) -> Result<Uuid, DocumentError> {
        let _guard = self.inner.gate.write().map_err(|_| poisoned())?;
        if operations.is_empty() {
            return Err(invalid("empty transaction"));
        }
        if operations.len() > 1000 {
            return Err(exceeded("transaction exceeds 1000 operations"));
        }
        let mut planner = Planner {
            store: self,
            pending: BTreeMap::new(),
        };
        for op in operations {
            match op {
                Operation::Put(edit) => planner.stage(edit)?,
                Operation::Place { edit, placement } => planner.place(edit, placement)?,
            }
        }
        self.apply_inner(author, reason, planner.pending.into_values().collect())
    }
    pub fn place(
        &self,
        author: &str,
        reason: &str,
        edit: Edit,
        placement: Placement,
    ) -> Result<Uuid, DocumentError> {
        self.transact(author, reason, vec![Operation::Place { edit, placement }])
    }
}
