# terra_incognita

Append-only epistemic store with first-class uncertainty, provenance, and temporal semantics.

## Core Philosophy

A database where uncertainty is not a problem to be eliminated but a fact of reality to be
modeled honestly. Every assertion knows what it is, where it came from, how confident we are,
and what contradicts it.

This is not a probabilistic inference engine. The system does not resolve conflicts
automatically. It stores epistemic state and delivers it honestly — to a human, to an agent,
to an analyst. Resolution is the caller's responsibility.

## Fundamental Invariants

These do not change. Everything else is negotiable.

**Append-only.** Nothing is updated in place. Nothing is deleted except under legal force
majeure (GDPR etc.) — and even then it is logged as a painful explicit operation, not a
routine one.

**Atomic assertions.** One assertion = one property of one entity. Each can be independently
confirmed, contradicted, or updated.

**Provenance everywhere.** Every assertion carries a source reference. No fact without origin.
No origin without timestamp.

**Open world.** Absence of an assertion does not mean the fact is false. It means we
don't know. This is different from NULL. This is different from false.

**Deletion is a new fact.** "X no longer exists as of t2" does not invalidate "X existed
at t1". Both are true. Both stay.

**Schema is explicit.** Entity types and attributes are registered explicitly before use.
Automatic schema creation on insert is forbidden. It leads to chaos.

## Data Model

Three concerns, kept separate:

**Schema registry** — what entity types exist, what properties are allowed for each type.
This is the contract. Strict. Explicit. Branch-local — each branch can extend the schema
independently, inheriting from its parent at the time of branching. (v0.1 had value types
Set/Range/Struct per property — removed in v0.2, all values are JSON.)

**Sources** — the raw origin of knowledge. Immutable after insert. A source said what it
said. If extraction was wrong, re-extract from the same source and replace the assertions.
Never modify the source itself.

**Assertions** — atomic claims about entities, each tied to a transaction, carrying
reasoning and a timestamp. The working layer. Queries go here.

**Relations** — connections between entities, stored as assertions. Hierarchies are dynamic
and emerge from relations — there is no separate hierarchy structure. An entity can belong
to multiple hierarchies simultaneously.

## Assertion Kinds (v0.1 — removed in v0.2)

v0.1 had fact/hypothesis distinction. Removed in v0.2 — see "v0.2 Rewrite" section.

v0.2 has only assertions. Uncertainty is expressed in assertion values and reasoning,
not in storage-level classification. One assertion per property per entity per transaction.

## Transactions

Transaction is the single mutation primitive. All writes — schema creation, property
attachment, entity introduction, assertions, visibility changes — happen inside a
transaction and commit atomically.

**tx_id** (UUID v7) replaces timestamps in all storage keys. UUID v7 is time-ordered
(same byte sort as i64 timestamps) but carries identity. Enables "show me state at tx X",
future rebase, cherry-pick.

**Processing order** within a transaction:
1. `properties` — create properties
2. `entity_types` — create types (may reference properties from step 1)
3. `attach` — attach properties to types
4. `hide` / `unhide` — visibility changes
5. `introduce` — create entities with assertions
6. `asserts` — assertions on existing/introduced entities

## Branches

Branches are the unit of isolated exploration. A branch inherits schema, entities, and
assertions from its parent at the moment of creation (`branch_point_tx`), then evolves
independently. No physical copying — reads walk the ancestry chain with temporal filtering.

**Main branch** (`Uuid::nil()`) is implicit, always exists, has no record stored.

**Ancestry chain**: `Vec<(Uuid, Uuid)>` = `[(branch_id, branch_point_tx)]` — precomputed
at creation. Max depth: 8. Temporal filtering uses UUID v7 byte comparison:
`tx_id <= branch_point_tx`.

**Schema inheritance**: A child branch sees all schema items created in ancestors before
its `branch_point_tx`. Writes always go to the current branch. Slug uniqueness is checked
across the entire ancestry chain.

**BranchRecord**: `{id, slug, reasoning, created_from_tx, ancestry}`. Parent is derived
from `created_from_tx` → transaction → `branch_id`. No separate `parent_id` field.

## Visibility

Visibility controls which items are in scope on a branch. Items can be hidden or unhidden
per branch via transactions.

**Item kinds**: Entity, EntityType, Property.

**Storage**: `branch_id(16) | tx_id(16) | item_kind(1) | item_id(16)` = 49 bytes.
Value: `1` = hidden, `0` = visible. Default (no record) = visible.

**Read filtering**: All read commands (`ListEntityTypes`, `GetEntityType`,
`ListProperties`, `ListEntities`, `GetEntity`) filter out hidden items.

**Write validation**: Transactions reject references to hidden items with distinct
errors (`EntityHidden`, `EntityTypeHidden`, `PropertyHidden`) separate from not-found.
Error messages include a hint: "exists but is hidden — use unhide to bring it into scope".

## Query Model

Queries return distributions, not facts. Multiple sources may assert different values for
the same property. The database returns all of them with their confidence and provenance.
The caller decides what to do with the distribution.

**Context-oriented queries** are the hard unsolved problem — how to retrieve what is
relevant to current reasoning without pulling everything. Do not design the API around
a solution that does not exist yet. Start with simple lookups. Let the hard problem
emerge from real usage.

## First Client: Code Companion Memory

A code companion that uses terra_incognita as its persistent memory layer.

Why this first:
- Immediate feedback loop — developer feels utility daily
- Real data, real query patterns, no synthetic benchmarks
- Multiple natural hierarchies — filesystem, modules, dependencies, call graph
- Provenance matters — "why do you think that?" has a real answer
- Confidence accumulates — agent gets smarter about the codebase over time
- Token reduction — structured context beats stuffing raw conversation history

## What This Is Not

**Not a vector database.** Vectors compress meaning and lose structure and provenance.

**Not a probabilistic inference engine.** Bayesian revision, Markov Logic Networks,
AGM belief revision — academically interesting, not the goal. The goal is honest
storage, not automatic resolution.

**Not an event sourcing system.** Similar philosophy but different semantics. Events
are things that happened in the system. Assertions are claims about the external world.

**Not Datomic.** Close in spirit but Datomic assumes one authoritative source of truth.
terra_incognita assumes multiple competing sources of partial truth.

## Open Problems

**Context-oriented queries.** How to retrieve what is relevant to current reasoning
without pulling everything. The right abstractions are unclear.

**Confidence aggregation.** Multiple sources assert the same property with different
confidence values. How to combine them? Source reliability is itself an assertion.
Circular but tractable.

**Hierarchical query performance.** Recursive traversal of dynamic relation-based
hierarchies at scale. Open problem — do not optimize prematurely.

## Ownership and Concurrency

Assume multi-threaded and async context. Design all long-lived types accordingly.

**No lifetimes on long-lived types.** Structs that persist beyond a single function
call must not carry lifetime parameters. Use `Arc<T>` for shared ownership instead
of `&'a T`. Lifetimes are acceptable only for short-lived borrows within a function
scope (iterators, builders, closures).

**No references for long-term storage.** Storing `&'a T` in a struct couples it to
the lender's lifetime and makes the type unusable across threads and async boundaries.
Use `Arc<T>` (or `Arc<Mutex<T>>` / `Arc<RwLock<T>>` when interior mutability is needed).

## Code Style

Docstrings (`///`) on all public items (structs, enums, functions, methods, traits).
This overrides any global instruction to skip docstrings.

## Commit Messages

Short and informative — describe the intent, not the diff. Do not mention changes
to CLAUDE.md, README, or other meta-files in commit messages.

## Code Organization

One entity — one file. Deep directory structure when needed. No god-files,
no "utils.rs", no bags of loosely related things. `mod.rs` files contain only
`mod` declarations and re-exports — no logic, no types, no functions.
All `impl` blocks for a type live in the file where the type is defined.
Do not spread `impl` across multiple files unless implementing a foreign trait.
Use explicit `use` imports, not `super::` paths.

## Stack

- Rust — core, API, validation layer
- RocksDB — single storage engine for everything (schema, assertions, branches, entities)
- serde + serde_json — serialization
- axum + tokio — HTTP server (terra-server)

## Crate Structure

- **terra-core** — domain logic v0.1 (legacy, being replaced by terra-core-v0_2)
- **terra-core-v0_2** — domain logic v0.2 (active development, see "v0.2 Rewrite" below)
- **terra-query** — transport-agnostic query dispatcher (bytes in → bytes out)
- **terra-server** — HTTP wrapper around terra-query (`POST /query`, YAML/JSON)
- **terra-cli** — minimal stdin-to-HTTP client

## Deployment

Single binary. Zero infrastructure. No Docker, no containers, no network services.

**When PostgreSQL** — when the server scenario arrives: multiple agents, shared memory,
network access. Migration is cheap. That is v2. Not now.

## API Commands

Commands are sent as YAML or JSON with a `command` field:

- **Writes** (single mutation primitive):
  - `transaction` — atomic: schema ops + visibility + entity introduction + assertions
  - `branch.create` — create branch with optional embedded transaction
- **Reads**:
  - `entity-type.list`, `entity-type.get` — schema types (visibility-filtered)
  - `property.list` — schema properties (visibility-filtered)
  - `entity.list`, `entity.get` — entities (visibility-filtered)
  - `branch.get`, `branch.list` — branches
  - `log.list` — fact log entries

## Non-Goals for v1

- Distributed deployment
- Replication
- Automatic conflict resolution
- Probabilistic query evaluation
- Inference engine

## Naming

**terra_incognita** — named for what it stores: the unknown, the uncertain, the
unresolved. On old maps, terra incognita marked places where knowledge ran out.
This database makes that uncertainty first-class.

## v0.2 Rewrite (terra-core-v0_2)

### Why

v0.1 (terra-core) works but has hardcoded structures that prevent terra from being
a general-purpose epistemic store. Anyone building an agent on top of terra must either
accept our naming (reasoning, question, answer, tasks) or fork the codebase. The goal
of v0.2 is to make terra configurable for different agent developers without forking.

### What changes from v0.1

**1. No more value types (Set, Range, Struct) — only JSON.**

v0.1 has `ValueType` enum with Set, Range, Struct. Each gets separate column families
(6 total: fact_set, fact_range, fact_struct, hyp_set, hyp_range, hyp_struct). In practice
all three store JSON bytes with identical key formats. No type-specific queries, no
indexing, no algebra. The only effect is complexity: PropertyValue enum, type matching
in writer/reader, parse_property_value guessing type from JSON shape.

v0.2: property values are `serde_json::Value`. One column family for assertions.
Schema properties have a slug and a description, no value type. Validation of value
shape is the caller's responsibility (or described in agent config, not in storage).

**2. No more fact/hypothesis distinction — only assertions.**

v0.1 separates facts and hypotheses into different column families with a special query
model: "latest fact + all hypotheses after it". The deliberation cycle
(hypotheses → fact → new hypotheses) is a complex concept that the LLM agent must
understand and apply correctly. In practice, deciding "is this a fact or a hypothesis?"
is itself an uncertain judgment — ironic for a system designed to model uncertainty.

v0.2: only assertions. Each assertion is a claim with a value and reasoning. Uncertainty
is expressed in the data itself (confidence fields, alternative values, hedging language
in reasoning). The agent writes what it knows and explains its confidence in reasoning.
One assertion per property per transaction (same constraint as v0.1 facts).

This eliminates: 2 separate writers, fact/hypothesis CF split, AssertionKind enum,
validate_no_conflicting_facts (becomes validate_no_conflicting_assertions),
the "latest fact + hypotheses after it" query model, and a significant chunk of the
system prompt explaining when to use facts vs hypotheses.

**3. Transaction metadata is dynamic, not hardcoded.**

v0.1 TransactionInput has fixed fields: reasoning (Value), question (Option<String>),
answer (Option<String>), commands (Vec<Value>). These are serialized into the transaction
record in RocksDB. An agent developer who wants "notes" instead of "reasoning" or
wants to add "confidence" must change Rust code.

v0.2: transaction metadata is `Map<String, Value>`. What fields exist, which are required,
and their types — defined in a project config file (YAML). The core validates metadata
against the config at write time. Default config ships with reasoning/question/answer
for backward compatibility, but the agent developer can replace it entirely.

**4. Tasks removed from core — replaced by managed types.**

v0.1 has a dedicated task subsystem: TaskStore, TaskIo, TaskRecord, TaskStatus enum,
2 column families (task_main, task_slug), visibility integration with ItemKind::Task,
and 3 transaction operations (tasks, update_tasks, close_tasks). This is ~500 lines
of code that duplicates entity storage patterns (versioned records, slug index,
branch-aware ancestry walk) with a hardcoded schema (goal, reasoning, context, kind,
notes, resolution, status: open/closed).

v0.2: managed types defined in project config. A managed type has a name, fields
(required/optional with types), and an optional lifecycle (states + transitions).
Tasks become one possible managed type in a default config:

```yaml
managed_types:
  task:
    fields:
      goal: { type: json, required: true }
      notes: { type: json }
      resolution: { type: json }
    lifecycle:
      states: [open, closed]
      initial: open
      transitions:
        open: [closed]
```

Storage: single pair of column families (managed_main, managed_slug) shared by all
managed types, with a type name hash prefix in the key:

```
managed_main: type_hash(16) | branch_id(16) | item_id(16) | tx_id(16) = 64 bytes
managed_slug: type_hash(16) | branch_id(16) | slug_bytes
```

No dynamic CF creation. New managed type in config = immediately writable.

**5. Simplified TransactionInput.**

v0.1:
```rust
pub struct TransactionInput {
    pub reasoning: serde_json::Value,
    pub question: Option<String>,
    pub answer: Option<String>,
    pub commands: Vec<serde_json::Value>,
    pub entity_types: Vec<CreateEntityType>,
    pub add_properties: Vec<AddProperties>,
    pub hide: HideUnhideInput,
    pub unhide: HideUnhideInput,
    pub introduce: Vec<IntroduceItem>,
    pub asserts: Vec<AssertItem>,
    pub tasks: Vec<TaskCreateItem>,
    pub update_tasks: Vec<TaskUpdateItem>,
    pub close_tasks: Vec<TaskCloseItem>,
}
```

v0.2:
```rust
pub struct TransactionInput {
    /// Dynamic metadata validated against project config.
    pub meta: Map<String, Value>,
    /// Schema operations.
    pub entity_types: Vec<CreateEntityType>,
    pub add_properties: Vec<AddProperties>,
    /// Visibility.
    pub hide: HideUnhideInput,
    pub unhide: HideUnhideInput,
    /// Data operations.
    pub introduce: Vec<IntroduceItem>,
    pub asserts: Vec<AssertItem>,
    /// Managed type operations (tasks, etc. — defined in config).
    pub managed: Map<String, Vec<ManagedOperation>>,
}
```

### What stays the same from v0.1

- RocksDB as storage engine
- `storage_key!` macro for fixed-size binary keys
- Branch system: ancestry chain, branch_point_tx, max depth 8, slug uniqueness
- Entity storage: entity_main, entity_slug CFs, versioned records
- Schema registry: entity types, properties, branch-aware with inheritance
- Visibility: hide/unhide per branch, ItemKind (but without Task variant)
- Transactions: atomic writes via WriteBatch, UUID v7 tx_id
- Append-only invariant, open-world semantics
- Processing order: schema ops → visibility → introduce → asserts

### How to work on v0.2

- **Reference**: terra-core (v0.1) is the reference implementation. Read it, port what fits.
- **Do not modify terra-core.** It stays as-is until v0.2 is ready to replace it.
- **Port order**: storage_key macro → store (open RocksDB) → schema registry →
  entities → visibility → assertions → managed types → transaction execution.
- **Tests**: port and adapt tests from terra-core. Each module should have tests before
  moving to the next.

### Column families in v0.2

| CF | Purpose | Key format |
|----|---------|------------|
| transactions | Transaction metadata | branch(16) \| tx(16) |
| entity_main | Entity records | branch(16) \| entity(16) \| tx(16) |
| entity_slug | Entity slug → UUID | branch(16) \| slug |
| branch_main | Branch records | branch(16) \| tx(16) |
| branch_slug | Branch slug → UUID | slug |
| schema_types | Entity types | branch(16) \| type(16) |
| schema_type_slug | Type slug → UUID | branch(16) \| slug |
| schema_props | Properties | branch(16) \| prop(16) |
| schema_prop_slug | Property slug → UUID | branch(16) \| slug |
| schema_attachments | Type-property links | branch(16) \| type(16) \| prop(16) |
| visibility | Hide/unhide state | branch(16) \| tx(16) \| kind(1) \| id(16) |
| assertions | All assertions (was 6 CFs) | branch(16) \| prop(16) \| tx(16) \| entry(16) \| entity(16) |
| assertion_log | Assertion log entries | entry(16) |
| managed_main | Managed type records | type_hash(16) \| branch(16) \| item(16) \| tx(16) |
| managed_slug | Managed type slugs | type_hash(16) \| branch(16) \| slug |

15 CFs (was 21 in v0.1: 19 original + 2 task CFs).
