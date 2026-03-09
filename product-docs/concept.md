# Concept

## What it is

A knowledge store where uncertainty is a first-class citizen.
Not a database of facts — a database of claims about the world,
each with a source, timestamp, and degree of confidence.

## Core idea

Every piece of knowledge is someone's assertion. Two sources can
contradict each other — and both remain in the system. Conflict
resolution is the consumer's responsibility, not the storage's.

## Facts and hypotheses

Assertions come in two kinds:

- **Hypothesis** — a tentative claim. Multiple hypotheses can coexist
  for the same property, representing the space of possibilities.
- **Fact** — a convergence point. "Based on available evidence, the
  value is X." A fact does not delete hypotheses — it marks a decision
  amid uncertainty. The hypotheses remain as the reasoning trail.

The deliberation cycle: hypotheses → fact → new hypotheses → new fact.
Each cycle narrows understanding while preserving the full history
of reasoning.

## Entities and types

An entity is just an identifier (UUID + slug). It has no fixed type.

"This entity is a track" is itself an assertion — a fact or a hypothesis.
A single entity can simultaneously be a track, an audio file, and a
licensing object — depending on the projection.

## Typed properties

Properties are typed:
- **Set** — membership assertions (contains / does not contain)
- **Range** — numeric or ordinal values (exact, interval, open-ended)
- **Struct** — arbitrary structured data

Schema is mandatory. You cannot write a property without declaring it
in the schema and attaching it to an entity type.

## Queries

The primary query returns the current picture: the latest facts for each
property and the count of open hypotheses. The answer to "what do we know,
and how much is still in question." Hypotheses can be expanded with a
separate query — when you need not just the state, but the space of
possibilities.

## Branching

A branch is a thought experiment: "assume A is a fact — what conclusions
follow?"

**Checkout** pins starting assumptions as facts on top of the parent branch.
Within the branch, all reasoning proceeds from these assumptions — conclusions
are written as facts (in the branch's context).

**Merge** brings the branch's conclusions back to the parent as hypotheses.
Because the original assumption is not yet confirmed at the parent level.
The branch's findings require review before becoming facts.

**Main = consensus.** Only confirmed facts live on main. Committing directly
to main means "we know this for certain, no reasoning needed." Possible,
but rare.

This is the scientific method as a workflow:
- Two branches — "what if jazz?" and "what if blues?" — isolated lines
  of reasoning that don't interfere with each other
- Merge = peer review: conclusions arrive as hypotheses, still need acceptance
- Merge conflict = genuine knowledge conflict, not a textual diff

This is likely the primary workflow rather than throwing hypotheses directly —
just as in git, you rarely commit straight to main.

## Invariants

- **Append-only.** Nothing is updated in place. Nothing is deleted.
- **Provenance.** Every assertion knows its source.
- **Open world.** Absence of an assertion ≠ false. It means we don't know.
- **Deletion is a new fact.** "X was deleted at t2" does not invalidate
  "X existed at t1". Both are true. Both stay.

## First target use case

Persistent memory for a code companion — an AI agent that uses
terra_incognita as its long-term knowledge store about a codebase.
Provenance, confidence, reasoning history — everything that raw chat
history lacks.
