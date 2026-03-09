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

**Schema registry** — what entity types exist, what attributes are allowed for each type,
what value types those attributes carry. This is the contract. Strict. Explicit.

**Sources** — the raw origin of knowledge. Immutable after insert. A source said what it
said. If extraction was wrong, re-extract from the same source and replace the assertions.
Never modify the source itself.

**Assertions** — atomic claims about entities, each tied to a source, carrying a confidence
value and a timestamp. The working layer. Queries go here.

**Relations** — connections between entities, stored as assertions. Hierarchies are dynamic
and emerge from relations — there is no separate hierarchy structure. An entity can belong
to multiple hierarchies simultaneously.

## Assertion Kinds

Every assertion has a kind: **hypothesis** or **fact**.

**Hypothesis** — a tentative claim. Multiple hypotheses can coexist for the same
property. They represent the space of possibilities under consideration.

**Fact** — a convergence point. "Based on available evidence, the position was X."
A fact does not delete hypotheses — it marks a decision amid uncertainty.
The hypotheses remain as the reasoning trail.

**Deliberation cycle.** The workflow is: hypotheses → fact → new hypotheses →
new fact. Each cycle narrows understanding while preserving the full history
of reasoning.

**Default query behavior.** A query for a property returns the latest fact in
the requested time interval, plus all hypotheses that follow it. This gives the caller
the current best understanding together with open questions. If no fact exists,
all hypotheses are returned.

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

## Code Organization

One entity — one file. Deep directory structure when needed. No god-files,
no "utils.rs", no bags of loosely related things.

## Stack

- Rust — core, API, validation layer
- SQLite — schema registry
- RocksDB — assertions and sources
- rusqlite — SQLite client
- serde + serde_json — serialization

## Deployment

Single binary. Zero infrastructure. No Docker, no containers, no network services.

**When PostgreSQL** — when the server scenario arrives: multiple agents, shared memory,
network access. Migration is cheap. That is v2. Not now.

## Non-Goals for v1

- Distributed deployment
- Replication
- Automatic conflict resolution
- Probabilistic query evaluation
- Inference engine
- Web API — internal library first

## Naming

**terra_incognita** — named for what it stores: the unknown, the uncertain, the
unresolved. On old maps, terra incognita marked places where knowledge ran out.
This database makes that uncertainty first-class.
