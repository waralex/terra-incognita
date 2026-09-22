//! Experimental document store. Open a separate database, never a legacy Terra DB.
//! Stable block IDs, immutable versions and atomically maintained current indexes.
mod placement;
mod records;
use crate::io::storage_key::StorageKey;
use crate::io::{DbError, TerraDb};
pub use placement::{Operation, Placement};
use records::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::{Arc, RwLock},
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Kind {
    Section,
    Text,
    ListItem {
        spread: bool,
    },
    /// None is a bullet list; Some(n) is an ordered list starting at n.
    List {
        start: Option<u32>,
        #[serde(default)]
        spread: bool,
    },
}
impl Kind {
    fn accepts(&self, child: &Kind) -> bool {
        match self {
            Self::Section => !matches!(child, Self::ListItem { .. }),
            Self::List { .. } => matches!(child, Self::ListItem { .. }),
            Self::ListItem { .. } => matches!(child, Self::Text | Self::List { .. }),
            Self::Text => false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskState {
    Todo,
    Done,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Content {
    pub parent: Option<Uuid>,
    /// Nonnegative sibling rank; equal ranks use block ID as deterministic tie-break.
    pub position: i64,
    pub kind: Kind,
    pub title: Option<String>,
    pub body: String,
    pub state: Option<TaskState>,
    /// Optional project entry-point label, independent of tree placement.
    #[serde(default)]
    pub entrypoint: Option<String>,
    pub deleted: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Block {
    pub id: Uuid,
    pub tx: Uuid,
    pub content: Content,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub id: Uuid,
    pub author: String,
    pub reason: String,
}
/// None means create-only, Some(tx) means compare-and-swap that exact head.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub id: Uuid,
    pub expected: Option<Uuid>,
    pub content: Content,
}
struct Inner {
    db: TerraDb,
    gate: RwLock<()>,
}
#[derive(Clone)]
pub struct DocumentStore {
    inner: Arc<Inner>,
}
#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("limit: {0}")]
    Limit(String),
    #[error(transparent)]
    Storage(#[from] DbError),
}
fn invalid(message: &str) -> DocumentError {
    DocumentError::InvalidInput(message.into())
}
fn exceeded(message: &str) -> DocumentError {
    DocumentError::Limit(message.into())
}
fn conflict(message: &str) -> DocumentError {
    DocumentError::Conflict(message.into())
}
fn poisoned() -> DocumentError {
    DocumentError::Storage(DbError::Storage("document lock poisoned".into()))
}

impl DocumentStore {
    pub fn open(path: &Path) -> Result<Self, DocumentError> {
        let db = TerraDb::builder(path)
            .with::<Head>()
            .with::<Version>()
            .with::<Child>()
            .with::<ChildVersion>()
            .with::<Tx>()
            .with::<Change>()
            .open()?;
        Ok(Self {
            inner: Arc::new(Inner {
                db,
                gate: RwLock::new(()),
            }),
        })
    }
    fn head(&self, id: Uuid) -> Result<Option<Block>, DocumentError> {
        Ok(self
            .inner
            .db
            .get::<Head>(&HeadKey { block: id })?
            .map(|e| e.value))
    }
    fn version(&self, id: Uuid, at: Option<Uuid>) -> Result<Option<Block>, DocumentError> {
        match at {
            None => self.head(id),
            Some(tx) => Ok(self
                .inner
                .db
                .scan_rev::<Version>(
                    &VersionKey::bound()
                        .with_prefix(|k| k.block = id)
                        .with_upper(|k| k.tx = tx),
                )?
                .next()
                .transpose()?
                .map(|e| e.value)),
        }
    }
    /// Includes tombstones so callers can explicitly restore with the correct revision.
    pub fn get(&self, id: Uuid, at: Option<Uuid>) -> Result<Option<Block>, DocumentError> {
        let _guard = self.inner.gate.read().map_err(|_| poisoned())?;
        self.version(id, at)
    }
    fn children_inner(&self, parent: Uuid, at: Option<Uuid>) -> Result<Vec<Block>, DocumentError> {
        self.children_bounded(parent, at, usize::MAX)
    }
    fn children_bounded(
        &self,
        parent: Uuid,
        at: Option<Uuid>,
        budget: usize,
    ) -> Result<Vec<Block>, DocumentError> {
        if self.version(parent, at)?.is_none_or(|b| b.content.deleted) {
            return Ok(vec![]);
        }
        let ids = if let Some(tx) = at {
            // Discover each distinct child using keys only, then seek directly
            // to its last membership at the requested transaction.
            let mut keys = self.inner.db.scan_keys::<ChildVersion>(
                &ChildVersionKey::bound().with_prefix(|k| k.parent = parent),
            )?;
            let mut ids = Vec::new();
            while let Some(key) = keys.next() {
                let child = key?.child;
                let bound = ChildVersionKey::bound()
                    .with_prefix(|k| {
                        k.parent = parent;
                        k.child = child;
                    })
                    .with_upper(|k| k.tx = tx);
                if let Some(entry) = self
                    .inner
                    .db
                    .scan_rev::<ChildVersion>(&bound)?
                    .next()
                    .transpose()?
                {
                    if entry.value.present {
                        if ids.len() == budget {
                            return Err(exceeded("children budget exceeded"));
                        }
                        ids.push(child);
                    }
                }
                // Skip every remaining version of this child. No synthetic
                // transaction boundary: UUID successor advances the child prefix.
                let Some(next) = child.as_u128().checked_add(1) else {
                    break;
                };
                keys.seek(&ChildVersionKey::bound().with_prefix(|k| {
                    k.parent = parent;
                    k.child = Uuid::from_u128(next);
                }));
            }
            ids
        } else {
            let mut ids = Vec::new();
            let keys = self.inner.db.scan_keys::<Child>(
                &ChildKey::bound()
                    .with_prefix(|k| k.parent = parent)
                    .with_lower(|k| k.position = 0),
            )?;
            for key in keys {
                let key = key?;
                if ids.len() == budget {
                    return Err(exceeded("children budget exceeded"));
                }
                ids.push(key.child);
            }
            ids
        };
        let mut result = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(b) = self.version(id, at)? {
                if !b.content.deleted {
                    result.push(b)
                }
            }
        }
        result.sort_by_key(|b| (b.content.position, b.id));
        Ok(result)
    }
    pub fn children(&self, parent: Uuid, at: Option<Uuid>) -> Result<Vec<Block>, DocumentError> {
        let _guard = self.inner.gate.read().map_err(|_| poisoned())?;
        self.children_inner(parent, at)
    }
    /// Read a bounded subtree under one read lock. Exceeding the node budget
    /// returns an error rather than silently presenting an incomplete tree.
    pub fn subtree(
        &self,
        root: Uuid,
        at: Option<Uuid>,
        depth: usize,
        max_nodes: usize,
    ) -> Result<Vec<Block>, DocumentError> {
        if depth > 64 || max_nodes == 0 || max_nodes > 10000 {
            return Err(exceeded("subtree requires depth <= 64 and 1–10000 nodes"));
        }
        let _guard = self.inner.gate.read().map_err(|_| poisoned())?;
        let Some(block) = self.version(root, at)? else {
            return Ok(vec![]);
        };
        if block.content.deleted {
            return Ok(vec![]);
        }
        let mut result = vec![];
        let mut stack = vec![(block, 0)];
        while let Some((block, level)) = stack.pop() {
            if result.len() == max_nodes {
                return Err(exceeded("subtree node budget exceeded"));
            }
            if level < depth {
                let remaining = max_nodes - result.len() - stack.len() - 1;
                let children = self.children_bounded(block.id, at, remaining)?;
                if result.len() + stack.len() + children.len() + 1 > max_nodes {
                    return Err(exceeded("subtree node budget exceeded"));
                }
                stack.extend(children.into_iter().rev().map(|b| (b, level + 1)));
            }
            result.push(block);
        }
        Ok(result)
    }

    pub fn history(
        &self,
        id: Uuid,
        before: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<Block>, DocumentError> {
        let _guard = self.inner.gate.read().map_err(|_| poisoned())?;
        if limit > 1000 {
            return Err(exceeded("history limit exceeds 1000"));
        }
        let bound = VersionKey::bound()
            .with_prefix(|k| k.block = id)
            .with_upper(|k| k.tx = before.unwrap_or(Uuid::max()));
        self.inner
            .db
            .scan_rev::<Version>(&bound)?
            .take(limit)
            .map(|e| e.map(|e| e.value).map_err(DocumentError::from))
            .collect()
    }
    /// Capture a durable read boundary; subsequent historical reads at this ID
    /// remain coherent even if another writer commits in between.
    pub fn latest_transaction(&self) -> Result<Option<Uuid>, DocumentError> {
        let _guard = self.inner.gate.read().map_err(|_| poisoned())?;
        self.inner
            .db
            .scan_rev::<Tx>(&TxKey::bound())?
            .next()
            .transpose()
            .map(|row| row.map(|r| r.key.tx))
            .map_err(Into::into)
    }

    pub fn transaction(&self, id: Uuid) -> Result<Option<Transaction>, DocumentError> {
        Ok(self.inner.db.get::<Tx>(&TxKey { tx: id })?.map(|e| e.value))
    }
    pub fn changed_blocks(&self, tx: Uuid) -> Result<Vec<Uuid>, DocumentError> {
        self.inner
            .db
            .scan::<Change>(&ChangeKey::bound().with_prefix(|k| k.tx = tx))?
            .map(|e| e.map(|e| e.key.block).map_err(DocumentError::from))
            .collect()
    }
    pub fn apply(
        &self,
        author: &str,
        reason: &str,
        edits: Vec<Edit>,
    ) -> Result<Uuid, DocumentError> {
        let _guard = self.inner.gate.write().map_err(|_| poisoned())?;
        self.apply_inner(author, reason, edits)
    }
    // Caller holds the store write lock across planning, validation and commit.
    fn apply_inner(
        &self,
        author: &str,
        reason: &str,
        edits: Vec<Edit>,
    ) -> Result<Uuid, DocumentError> {
        if author.trim().is_empty() || reason.trim().is_empty() || edits.is_empty() {
            return Err(invalid("author, reason and 1–1000 edits required"));
        }
        if edits.len() > 1000 {
            return Err(exceeded("transaction exceeds 1000 changed blocks"));
        }
        let mut pending = BTreeMap::new();
        let mut old = BTreeMap::new();
        for edit in edits {
            if edit.id == Uuid::nil() || edit.id == Uuid::max() || edit.content.position < 0 {
                return Err(invalid("invalid block ID or negative position"));
            }
            let previous = self.head(edit.id)?;
            if previous.as_ref().map(|b| b.tx) != edit.expected {
                return Err(conflict("stale block revision"));
            }
            if pending.insert(edit.id, edit.content).is_some() {
                return Err(invalid("duplicate block edit"));
            }
            old.insert(edit.id, previous);
        }
        // Validate final topology, including multiple moves/creations in one transaction.
        for (&id, content) in &pending {
            if content.deleted {
                for child in self.children_inner(id, None)? {
                    let final_child = pending.get(&child.id).unwrap_or(&child.content);
                    if !final_child.deleted && final_child.parent == Some(id) {
                        return Err(invalid("cannot delete a section with live children"));
                    }
                }
                continue;
            }
            if content.kind != Kind::Section && content.title.is_some() {
                return Err(invalid("title belongs only to a section"));
            }
            if content.kind != Kind::Text && !content.body.is_empty() {
                return Err(invalid("body belongs only to a text block"));
            }
            if content.state.is_some() && !matches!(content.kind, Kind::ListItem { .. }) {
                return Err(invalid("checkbox state belongs to a list item"));
            }
            if content.parent.is_none() && content.kind != Kind::Section {
                return Err(invalid("only sections can be roots"));
            }
            if matches!(content.kind, Kind::List { start: Some(n), .. } if n > 999_999_999) {
                return Err(invalid("ordered list start exceeds Markdown range"));
            }
            let mut seen = HashSet::from([id]);
            let mut child_kind = content.kind.clone();
            let mut parent = content.parent;
            while let Some(p) = parent {
                if !seen.insert(p) {
                    return Err(invalid("parent cycle"));
                }
                let ancestor = match pending.get(&p) {
                    Some(c) => c.clone(),
                    None => {
                        self.head(p)?
                            .ok_or_else(|| invalid("missing parent"))?
                            .content
                    }
                };
                if ancestor.deleted || !ancestor.kind.accepts(&child_kind) {
                    return Err(invalid("invalid parent/child block kinds"));
                }
                child_kind = ancestor.kind.clone();
                parent = ancestor.parent;
            }
            if old
                .get(&id)
                .and_then(|b| b.as_ref())
                .is_some_and(|b| b.content.kind == content.kind)
            {
                continue;
            }
            for child in self.children_inner(id, None)? {
                let final_child = pending.get(&child.id).unwrap_or(&child.content);
                if !final_child.deleted
                    && final_child.parent == Some(id)
                    && !content.kind.accepts(&final_child.kind)
                {
                    return Err(invalid("new kind cannot contain existing children"));
                }
            }
        }
        let last = self
            .inner
            .db
            .scan_rev::<Tx>(&TxKey::bound())?
            .next()
            .transpose()?
            .map(|t| t.key.tx)
            .unwrap_or(Uuid::nil());
        let tx = Uuid::from_u128(
            Uuid::now_v7().as_u128().max(
                last.as_u128()
                    .checked_add(1)
                    .ok_or_else(|| exceeded("transaction ID exhausted"))?,
            ),
        );
        let mut batch = self.inner.db.batch();
        batch.put(&Tx {
            key: TxKey { tx },
            value: Transaction {
                id: tx,
                author: author.into(),
                reason: reason.into(),
            },
        })?;
        for (id, content) in pending {
            let previous = old.remove(&id).flatten();
            let old_link = previous
                .as_ref()
                .filter(|b| !b.content.deleted)
                .and_then(|b| b.content.parent.map(|p| (p, b.content.position)));
            let new_link = if content.deleted {
                None
            } else {
                content.parent.map(|p| (p, content.position))
            };
            if old_link != new_link {
                if let Some((parent, position)) = old_link {
                    batch.delete::<Child>(&ChildKey {
                        parent,
                        position,
                        child: id,
                    })?;
                    batch.put(&ChildVersion {
                        key: ChildVersionKey {
                            parent,
                            child: id,
                            tx,
                        },
                        value: Membership {
                            present: false,
                            position,
                        },
                    })?;
                }
                if let Some((parent, position)) = new_link {
                    batch.put(&Child {
                        key: ChildKey {
                            parent,
                            position,
                            child: id,
                        },
                        value: Marker {},
                    })?;
                    batch.put(&ChildVersion {
                        key: ChildVersionKey {
                            parent,
                            child: id,
                            tx,
                        },
                        value: Membership {
                            present: true,
                            position,
                        },
                    })?;
                }
            }
            let block = Block { id, tx, content };
            batch.put(&Head {
                key: HeadKey { block: id },
                value: block.clone(),
            })?;
            batch.put(&Version {
                key: VersionKey { block: id, tx },
                value: block,
            })?;
            batch.put(&Change {
                key: ChangeKey { tx, block: id },
                value: Marker {},
            })?;
        }
        batch.commit()?;
        Ok(tx)
    }
}

#[cfg(test)]
mod tests;
