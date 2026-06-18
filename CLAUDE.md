# terra_incognita

Append-only epistemic store with first-class uncertainty, provenance, and
branching. A database of claims about the world — every assertion knows
what it is, who said it, when, and why.

## Core Philosophy

terra stores epistemic state and delivers it honestly. It does not
resolve conflicts automatically — multiple assertions on the same
property coexist and are returned as a distribution. Resolution is the
caller's responsibility.

## Fundamental Invariants

- **Append-only.** Nothing is updated in place. Nothing is deleted
  except under legal force majeure (GDPR etc.).
- **Atomic assertions.** One assertion = one property of one entity.
- **Provenance everywhere.** Every assertion is tied to a transaction
  and carries reasoning.
- **Open world.** Absence of an assertion ≠ false. It means we don't
  know.
- **Deletion is a new fact.** "X no longer exists after tx2" does not
  invalidate "X existed at tx1". Both are true. Both stay.
- **Explicit contracts for metadata.** Transaction metadata fields and
  managed types are declared in `schema.yaml`. Entity properties are
  open — consistency there is the caller's problem.

## Data Model

### Entities

Addressed by a slug (a human-readable identifier, unique within the
branch's ancestry). Carries a description and a set of property values.

No runtime entity types, no runtime property registry. Property slugs
are free-form at write time.

### Assertions

An atomic claim: (entity, property) = value, with reasoning and
provenance (tx_id + branch + change_id). Stored in the `assertions`
CF. Multiple assertions for the same property coexist — terra does
not auto-resolve.

Optionally carries an epistemic `status` (per `assertion_statuses` in
`schema.yaml`) — see **Assertion statuses** below. `None` when statuses
are not configured.

### Assertion statuses

Opt-in epistemic status on assertions (e.g. `fact` / `hypothesis` /
`observation`), declared under `assertion_statuses` in `schema.yaml`
with a `terminal` status and a `default`. Status is set per
entity-change (alongside `reasoning`) and copied onto every assertion
of that change.

Snapshot layering: per property, the latest **terminal** assertion is
the baseline; non-terminal assertions made after it are layered on top
(newest-first); everything older than the latest terminal is
consolidated away. A property with no terminal returns all its
overlays. A retraction (`null`) is terminal. So a single property can
appear multiple times in a snapshot. When `assertion_statuses` is
absent, snapshots are plain latest-wins and `status` is always `None`.

### Entity changes

A group of assertions on one entity made by a single transaction.
Carries its own meta (validated per `entity_change_meta`), typically
including `reasoning`. Referenced from each assertion by `change_id`.

### Transactions

The single mutation primitive. Carries its own dynamic meta (validated
per `transaction_meta`) — transaction-level reasoning lives here.

`tx_id` is a UUID v7: globally unique **and** chronologically ordered.
The timestamp is extracted from the tx_id on read; it is not stored
separately.

**Processing order** inside a transaction (enforced by the executor):

1. Create entities
2. Update entities (new assertions on existing entities)
3. Create managed items
4. Update managed items
5. Delete entities (soft-delete with reasoning)
6. Explicit touches (override auto-touches from create/update)

### Touching

Explicit "this entity is relevant to this transaction" signal, with a
reasoning string. Used by agents to mark context without changing
anything. Touches written in the explicit bucket override auto-touches
produced by create/update on the same slug.

### Deletion

Soft-delete: the entity is marked as deleted with reasoning, but prior
assertions stay queryable. An entity previously deleted may be
re-created with the same slug later.

### Managed types

Typed versioned records with optional lifecycle (e.g. `rule` with
states `draft` / `active` / `rejected` / `promoted`). Declared in
`schema.yaml` under `managed_types`.

All managed types share a single CF; the type name is part of the key.
Declaring a new managed type in the schema makes it immediately
writable — no code changes, no migration.

`managed.list` filters items by their type's `visible` lifecycle
states.

### Branches

Git-like isolated exploration. A child branch inherits everything its
parent had at the fork point (identified by `created_from_tx`), then
evolves independently. Branch creation is cheap: no data duplication —
reads walk the ancestry chain.

- Main branch is implicit (slug `"main"`), has no stored record, and
  its ancestry is empty.
- A non-main branch's record stores `parent_branch_slug` +
  `created_from_tx`. Ancestry is computed at context-load time, not
  stored as a field.
- Max ancestry depth bounded by project config (default 8).
- Slug uniqueness is checked across the entire ancestry chain.

## Meta vs reasoning (asymmetry to remember)

`meta` is a schema-validated bag of fields. `reasoning` is the
conventional field inside meta. But not every operation has meta —
touch and delete carry `reasoning` at the operation level instead.

**Where `reasoning` lives on write:**

| Operation | Field |
|---|---|
| Transaction | `transaction.meta.reasoning` — per `transaction_meta` |
| Entity create/update | `entity.meta.reasoning` — per `entity_change_meta` |
| Branch checkout | `branch.meta.reasoning` — per `branch_meta` |
| Touch | `touch.reasoning` at op level (no meta) |
| Delete | `delete.reasoning` at op level (no meta) |

**Where `reasoning` appears on read (`TxMeta.reasoning`):**

- Property's `context.reasoning` → **populated** with the assertion's
  reasoning (= the entity-change reasoning captured at write time).
- Entity / transaction / branch / managed `context.reasoning` →
  **always None** (skipped from serialization).

Transaction-level reasoning is available through the transaction's
`meta` block, not through any `context`.

`TxMeta.status` follows the same shape: **populated** on a property's
`context` (resolved per `assertion_statuses`), **always None** on
entity / transaction / branch / managed contexts and when statuses are
not configured.

**Entity `meta` on read also depends on the command:**

- `transaction.get` → entity `meta` is the entity-change meta recorded
  by that transaction (includes reasoning).
- `entities.touched` / `entities.similar` → entity `meta` is always
  empty. A snapshot reflects current state, which is the union of
  many entity-change metas — there is no single one to return.
  Per-property reasoning is still available through each property's
  `context`.

## Transaction input shape

```rust
pub struct TransactionInput {
    meta: Map<String, Value>,              // transaction_meta
    create_entities: Vec<Entity>,
    update_entities: Vec<Entity>,
    create_managed: Vec<Managed>,
    update_managed: Vec<Managed>,
    delete_entities: Vec<DeleteItem>,      // { entity, reasoning }
    touched: Vec<TouchItem>,               // { entity, reasoning }
}
```

`Entity` carries `slug`, optional `description`, `properties:
Vec<PropertyValue>`, `meta: Map<String, Value>` (per
`entity_change_meta`), and optional `status: Option<String>` (per
`assertion_statuses`).

There is no schema mutation in transactions. Schema lives only in
`schema.yaml`.

## HTTP commands

All go to `POST /query` with a `command` field + optional `branch`
(default `"main"`) and command-specific body fields flattened at the
top level. JSON or YAML selected by `Content-Type`.

- Writes: `transaction`, `checkout`
- Reads: `transactions.list`, `transaction.get`, `entities.touched`,
  `entities.similar`, `entities.grep`, `entity.history`, `branch.get`,
  `managed.list`

`transaction.get` is **cross-branch by design** (like `git show <sha>`):
a `tx_id` from any branch can be fetched regardless of the envelope's
`branch`.

Error response: `{ error: <message>, kind: <stable kind> }`. Kinds:
`parse_error`, `invalid_slug`, `validation_error`, `unknown_command`,
`not_found`, `conflict`, `storage_error`, `serialize_error`.

## Storage (RocksDB)

### Column families (10)

| CF | Key | Purpose |
|---|---|---|
| `transactions` | `branch(16) \| tx_id(16)` | Transaction meta |
| `transaction_log` | `tx_id(16)` | Denormalized summary of what a tx touched (global) |
| `entity_main` | `branch(16) \| entity(16) \| tx_id(16)` | Entity records (versioned) |
| `entity_changes` | `change_id(16)` | Entity-change meta (global, append-only) |
| `assertions` | `branch(16) \| entity(16) \| prop(16) \| tx_id(16)` | All assertions |
| `branch_main` | `branch(16)` | Branch records (not versioned) |
| `managed_main` | `branch(16) \| type_name(16) \| item(16) \| tx_id(16)` | All managed items (shared CF) |
| `touched` | `branch(16) \| tx_id(16) \| entity(16)` | Touched entities per tx |
| `visibility` | `branch(16) \| tx_id(16) \| item_kind(16) \| item_id(16)` | Hide/unhide state (internal, not exposed via public API yet) |
| `embeddings` | branch-scoped | Entity embeddings (optional, ONNX) |

Slugs are hashed to 16-byte UUIDs in keys; there is no separate
slug-index CF. The full slug is preserved in the value where needed.

### Key layout convention

Versioned keys (entity, assertion, managed, ...) share the pattern
`branch | ... middle ... | tx_id`. Branch first enables ancestry-scoped
scans; `tx_id` last enables reverse-scan-to-latest with UUID v7
byte-comparison.

See `io/storage_key.rs` (`storage_key!` macro) and
`store/versioned_key.rs` (`versioned_key!` macro).

## Project config

Two YAML files on disk:

- `terra-server.yaml` — `port`, `project_config_path`,
  `embed_model_dir` (optional, requires `onnx` feature).
- `project.yaml` — `data_dir`, `schema_path`.

`schema.yaml` sections:

```yaml
transaction_meta:
  reasoning: { type: text, required: true }

entity_change_meta:
  reasoning: { type: text, required: true }

branch_meta:
  reasoning: { type: text, required: true }

assertion_statuses:          # optional
  values: [fact, hypothesis, observation]
  terminal: fact
  default: observation

managed_types:
  rule:
    fields:
      content:   { type: text, required: true }
      rationale: { type: text }
    lifecycle:
      initial: draft
      states: [draft, active, rejected, promoted]
      visible: [draft, active]
```

There are no top-level `entity_types` or `properties` sections. The
entity model is open.

## Stack

- Rust (workspace, edition 2021)
- RocksDB — single storage engine
- axum + tokio — HTTP server (`terra-server`)
- Optional ONNX Runtime — embedding-backed similarity

Single binary. Zero infrastructure. No Docker, no network dependencies.

## Crate structure

- `terra-core` — domain logic, storage, command execution. Library
  entry: `Terra::open(path, config, schema, embedder)` and
  `terra.execute(branch, input)`.
- `terra-server` — axum-based HTTP server over terra-core. Single
  endpoint `POST /query`.
- `demo-terra-client` (TypeScript, not a workspace crate) — an
  exploratory POC that wires an LLM to terra-server. Not a reference
  integration.

## Code Style

- `///` docstrings on all public items (structs, enums, functions,
  methods, traits). This overrides any global instruction to skip
  docstrings.
- Minimal comments otherwise — only where logic isn't self-evident.
- One entity per file. Deep directory structure when needed. No
  god-files, no `utils.rs`, no bags of loosely related things.
- `mod.rs` files contain only `mod` declarations and re-exports —
  no logic, no types, no functions.
- All `impl` blocks for a type live in the file where the type is
  defined. Do not spread `impl` across files unless implementing a
  foreign trait.
- Explicit `use` imports, not `super::` paths.

## Ownership and Concurrency

Assume multi-threaded and async context.

- **No lifetimes on long-lived types.** Structs that persist beyond a
  single function call must not carry lifetime parameters. Use
  `Arc<T>` for shared ownership instead of `&'a T`. Lifetimes are
  acceptable only for short-lived borrows within a function scope
  (iterators, builders, closures).
- **No references for long-term storage.** Storing `&'a T` in a struct
  couples it to the lender's lifetime and makes the type unusable
  across threads and async boundaries. Use `Arc<T>` (or
  `Arc<Mutex<T>>` / `Arc<RwLock<T>>` when interior mutability is
  needed).

## Commit Messages

Short and informative — describe the intent, not the diff. Style from
this repo's log: `<scope>: <lowercase description>`. Do not mention
changes to `CLAUDE.md`, README, or other meta-files in commit messages.

## Non-goals

- General-purpose DBMS
- Vector database (embeddings are a convenience, not the primary model)
- Probabilistic inference / automatic conflict resolution
- Event-sourcing framework (events describe what happened in the
  system; assertions are claims about the outside world)
- Datomic clone (Datomic assumes one authoritative source; terra
  assumes multiple competing partial sources)
- Distributed deployment, replication, clustering — single binary only

## Open problems

- **Context-oriented queries.** How to retrieve what is relevant to
  current reasoning without pulling everything. Right abstractions
  unclear.
- **Confidence aggregation.** Multiple sources assert the same
  property with different confidence. How to combine? Source
  reliability is itself an assertion — circular but tractable.
- **Hierarchical query performance.** Recursive traversal of
  dynamic relation-based hierarchies at scale. Do not optimize
  prematurely.

## Naming

terra_incognita — named for what it stores: the unknown, the
uncertain, the unresolved. On old maps, terra incognita marked places
where knowledge ran out. This database makes that uncertainty
first-class.
