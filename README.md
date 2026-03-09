# terra_incognita

An append-only knowledge store where uncertainty is a first-class citizen.

Not a database of facts — a database of *claims about the world*, each carrying
its source, timestamp, and degree of confidence. Two sources can contradict each
other and both remain in the system. Conflict resolution is the consumer's
responsibility, not the storage's.

## Core idea

Every piece of knowledge is someone's assertion. Traditional databases force you
to pick one truth. terra_incognita stores them all — and remembers who said what,
when, and how sure they were.

### Facts and hypotheses

Assertions come in two kinds:

- **Hypothesis** — a tentative claim. Multiple hypotheses can coexist for the same
  property, representing the space of possibilities under consideration.
- **Fact** — a convergence point. "Based on available evidence, the value is X."
  A fact does not delete hypotheses — it marks a decision amid uncertainty.
  The hypotheses remain as the reasoning trail.

The deliberation cycle: hypotheses → fact → new hypotheses → new fact.
Each cycle narrows understanding while preserving the full history of reasoning.

### Entities and types

An entity is just an identifier (UUID + slug). It has no fixed type.

"This entity is a track" is itself an assertion — a fact or a hypothesis.
A single entity can simultaneously be a track, an audio file, and a licensing
object — depending on the projection.

### Typed properties

Properties are typed:
- **Set** — membership assertions (contains / does not contain)
- **Range** — numeric or ordinal values (exact, interval, open-ended)
- **Struct** — arbitrary structured data

Schema is mandatory. You cannot write a property without declaring it in the
schema and attaching it to an entity type.

### Queries

The primary query returns the current picture: the latest facts for each property
and the count of open hypotheses. The answer to "what do we know, and how much is
still in question." Hypotheses can be expanded with a separate query — when you
need not just the state, but the space of possibilities.

### Invariants

- **Append-only.** Nothing is updated in place. Nothing is deleted.
- **Provenance.** Every assertion knows its source.
- **Open world.** Absence of an assertion ≠ false. It means we don't know.
- **Deletion is a new fact.** "X was deleted at t2" does not invalidate
  "X existed at t1". Both are true. Both stay.

## Status

**Early development.** The storage layer (schema registry, assertion log,
typed columns, entity management) is taking shape. There is no public API,
no query engine, no CLI — just the foundation.

This is a design-driven project. The architecture is being built carefully,
one layer at a time. If you find the idea interesting, watch the repo —
things will move fast once the foundation is solid.

## First target use case

Persistent memory for a code companion — an AI agent that uses terra_incognita
as its long-term knowledge store about a codebase. Provenance, confidence,
reasoning history — everything that raw chat history lacks.

## Stack

- Rust
- RocksDB (assertions, entities)
- SQLite (schema registry)

Single binary. Zero infrastructure.

## License

TBD
