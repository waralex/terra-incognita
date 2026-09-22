# Document storage

`terra_core::documents::DocumentStore` is a separate storage model using the
existing typed RocksDB I/O. Open it on a **new, dedicated database directory**.
Current project memory uses this store through the shared document service.

## Identity and transactions

Blocks have stable UUIDs. Content contains parent, nonnegative sibling position,
kind, optional title, Markdown body, optional checkbox state and deletion flag.
Moving a section changes one block, not its descendants. Links can retain the
same block ID after a move; link parsing and backlinks are not implemented here.

`apply(author, reason, edits)` requires exact expected head revisions; `None`
means create-only. IDs cannot be reused after deletion without that revision.
The final topology is validated before any writes: parents must be live containers with compatible child kinds,
cycles are rejected, and deletion/type changes cannot strand live children.
Multiple creations and moves can be combined in one atomic batch. Deleting an
entire subtree requires explicit edits for its nodes.

A shared read/write lock serializes validation and commit across store clones;
compound reads use that lock too. RocksDB owns the directory exclusively. No raw
DB handle is exposed by this API. Transaction UUIDs are monotonically increased,
including across reopen and wall-clock rollback. The UUID timestamp is therefore
logical ordering metadata, not an independent measurement of wall time.

## Column families

- `doc_blocks`: block UUID -> complete latest version, including tombstones.
- `doc_block_versions`: block UUID, transaction UUID -> immutable complete version.
- `doc_children`: parent UUID, nonnegative i64 position, child UUID -> empty marker.
- `doc_child_versions`: parent UUID, child UUID, transaction UUID -> membership and position.
- `doc_transactions`: transaction UUID -> author and reason.
- `doc_transaction_changes`: transaction UUID, block UUID -> empty marker.

Current index rows can be physically replaced/deleted. Historical rows are never
removed. All six families are updated in the same RocksDB write batch. Moving a
block between parents records removal and insertion; reordering within a parent
records its final position at that transaction.

## Reads and costs

- Current block: one point get.
- Historical block: reverse seek in the block's version range, bounded by time.
- Current children: one parent range scan and one point get per child.
- Historical children: discover one key per ever-associated child, skip its
  remaining history using a child-prefix seek, and reverse-seek its last membership
  at the requested time. Read block versions only for members present then.
- Block history: bounded reverse scan (inclusive cursor, maximum 1000 rows).
- Subtree: depth-first adjacency traversal; explicit depth and node budgets.

Historical membership performs one key read and one reverse seek per child ever
associated with the parent, rather than decoding all membership versions. This
still depends on historical child count, not just visible children. RocksDB seek
cost and physical I/O depend on its indexes, cache and LSM layout; this is not a
constant-time or wall-clock performance guarantee. Children are sorted by position then
ID; equal positions are legal. No materialized full paths or global branches.

## Layer boundaries

This Rust API stores blocks and versions. [The Markdown adapter](document-markdown.md) handles parsing and rendering; [the MCP service](document-mcp.md) handles project access, agent operations and derived links. The engine does not implement Markdown link indexes or independent subtree copies. Existing databases are not migrated automatically.

## Markdown containers and relative placement

`Kind::Section` contains sections, text and lists. `Kind::List { start: None }`
is a bullet list; `Some(n)` is an ordered list (Markdown's 0–999999999 range).
Lists contain only `ListItem`; each item can contain text and nested lists. Text
is a leaf. Only sections can be roots; checkbox state belongs to list items.
Title is allowed only on Section; a nonempty body only on Text. Section and
ListItem prose must live in ordered Text children; List has no own text. These
invariants are checked on every live write, before commit.
Multiple paragraphs may remain together in one text body. Displayed ordered-list
numbers are computed from sibling order and start, never stored as positions.

`place(author, reason, edit, Placement)` accepts First/Last within a parent or
Before/After an anchor with its expected revision (`Some(tx)` for existing
blocks, `None` for an anchor created earlier in this transaction). It overrides parent/position
in the supplied edit; the block's own expected revision still applies. Planning
and commit hold the same write lock. Missing, deleted, stale or self anchors are
rejected. Root placement uses ordinary apply; relative placement requires a parent.

Positions use gaps (1024 for append and rebalance, midpoint for insertion).
When a gap is exhausted or equal ranks prevent insertion, siblings are re-ranked
in one transaction, preserving IDs, text and historical order. Any adjusted
sibling receives a new revision, intentionally invalidating stale edits to it.
Common placement uses directional neighbor seeks on index keys, merged with staged
positions. It does not read every sibling body and works above 1000 siblings.
Only a required full rebalance is limited to 1000 changed blocks (including the
inserted block); refusal is atomic. Rebalance loads bodies only for changed ranks.

`transact(author, reason, operations)` is the common atomic command boundary.
Operations are `Put(Edit)` and `Place { edit, placement }`. A private planner holds
an overlay under the store write lock: later commands see earlier creations,
moves and rank repairs. It validates final topology and invokes `apply_inner`
once. No writes escape on a planning/validation error. Maximum 1000 operations
and 1000 distinct changed blocks, including automatic rank repairs.

All expected revisions refer to committed heads before the transaction, even
when an earlier operation changed that block in the overlay. Each Put is a full
content replacement; a later Put of the same block replaces its earlier planned
content, including position. Anchor lookup uses its latest planned position.
First/Last choose the current parent contents under the lock, not an externally
pinned document revision. `place` is a one-operation convenience wrapper;
`apply` remains the lower-level full-state batch API.

`DocumentError` distinguishes Conflict, InvalidInput, Limit and Storage errors.
Adapters need not parse strings to decide whether a stale read needs refreshing.
This does not imply automatic retry of conflicts.

Subtree budgets are checked while discovering child IDs/memberships, before
loading their bodies. The first excess live member causes an error. Historical
reads may still examine absent memberships of formerly attached children. This
bounds the number of loaded blocks, not bytes in one block's body. Public
`children` itself remains an unpaginated API.
