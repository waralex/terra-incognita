# Concepts

This file explains the concepts behind terra's data model — what
the pieces are and how they relate. It is a conceptual reference,
not an API reference. Each concept is described in just enough depth
to place it in the overall picture.

Read top to bottom. Later concepts build on earlier ones.

## Transactions

The only way to write. An atomic batch of operations, committed
together or not at all; and a self-contained record that carries
project-defined meta (by default: `reasoning`). Identified by `tx_id`,
a UUID v7 — unique and chronologically ordered at once.

No mutable slots anywhere: every write is versioned by tx_id. What
"the current value of X" returns is a projection over the history
of transactions that touched X. State as of any past transaction is
reachable through the same projection capped earlier.

## Branches

Every transaction lives on a branch. The default branch is `main`,
which is always implicit.

`checkout` creates a new branch from a **branch point** in the
parent's history (defaults to the parent's latest transaction) and
atomically commits a first transaction on it. A branch cannot exist
with zero transactions.

A child sees the parent's history up to the branch point, and its
own history afterwards; the parent does not see the child. Branches
are isolated by construction.

## Entities

Terra's primary stored object. An entity is a container for
properties, identified by a slug (a human-readable, unique name)
and carrying an optional free-form description. The property list
is open — there is no runtime property registry and no predeclared
types; any property slug is valid at write time, and property values
are arbitrary JSON.

A single transaction can create a new entity (with any subset of
its properties) or update an existing one — adding new properties
or asserting new values for existing ones. An update only touches
the properties it mentions; everything else keeps its prior value.

This makes each property independent: two updates can touch
different subsets, and the entity's state at any moment is the
latest value asserted for each property, possibly each from a
different transaction.

## Managed items

Besides entities, terra stores **managed items** — objects with a
structure defined up front in `schema.yaml`. Tasks, notes, rules,
decisions: whatever a project wants to track in a typed, structured
form rather than as an open property bag.

Each managed type declares its fields and, optionally, a lifecycle
— a state machine of allowed states (e.g. `draft`, `active`,
`closed`) with an initial state. Instances of a type are managed
items; they live on a branch, are created and updated inside
transactions, and their state at any past transaction is reachable
by the same projection rules that apply to entities.

The difference from entities is granularity: a managed item changes
atomically at the whole-record level. Each write produces one
complete new version; fields do not have independent histories —
they move together, as a single record.

## Touching

When a transaction commits, terra records which entities it
**touched** — a separate index of "entities relevant to this
transaction." Two sources feed this index:

- **Auto-touches.** Every entity created or updated in the
  transaction is automatically marked as touched.
- **Explicit touches.** The transaction may list additional
  entities that were *considered* relevant — read from, reasoned
  about, referenced — without being changed.

Touches are entity-only; managed items are not tracked here.

This index answers "what has the agent been paying attention to"
across time, and is the foundation for building agent context
(`entities.touched` returns a reverse-chronological window).

## Deletion

terra's append-only invariant applies to deletion too: nothing is
ever erased. "Deletion" is a new record saying "after this
transaction, X is no longer current."

Two mechanisms:

- **Entity deletion.** A `delete` operation in a transaction marks
  an entity as no longer existing, with reasoning. After this
  transaction the entity is excluded from snapshot reads; its prior
  assertions stay in history, and reading at any `at_tx` before the
  deletion still sees a live entity.
- **Property retraction.** Writing `null` as a property value
  retracts just that property. The property disappears from snapshot
  reads, while earlier values remain visible in history.

An entity that has been deleted may later be created again with the
same slug. The new life and the old history coexist; the re-creation
does not undo or rewrite the deletion.

## A concrete transaction

Enough abstraction — here is what a real transaction looks like.
The scenario: an agent is in a conversation with a user named
Alice, and records one turn.

```yaml
command: transaction
branch: main

meta:
  reasoning: "ingesting this turn of conversation with Alice"
  question: "I moved to Berlin last month. Starting a climbing project with Bob."
  answer: "Got it. Noted your move to Berlin in March 2026 and the climbing project with Bob."

create:
  - slug: climbing-project
    description: "a side project Alice is starting with Bob"
    properties:
      - { property: status, value: "idea" }
      - { property: participants, value: ["alice", "bob"] }
    meta:
      reasoning: "Alice described this project in today's message"

update:
  - slug: alice
    properties:
      - { property: city, value: "Berlin" }
      - { property: moved_at, value: "2026-03" }
    meta:
      reasoning: "Alice said she moved last month; current date is 2026-04-22"

create_managed:
  - type_name: rule
    slug: prefer-absolute-dates
    state: draft
    fields:
      content: "always record dates as absolute (2026-03), never relative (last month)"
      rationale: "relative dates rot as conversations age; absolutes stay correct"

touch:
  - entity: bob
    reasoning: "Alice mentioned Bob as a collaborator; not updating him yet"
```

### Envelope

`command: transaction` identifies the operation; `branch: main`
targets the main line of history. `branch` defaults to `main` if
omitted.

### `meta`

The transaction's own meta — fields defined by `transaction_meta`
in `schema.yaml`. The default setup takes `reasoning` (required)
plus `question` and `answer` — a template well-suited to capturing
a single conversational turn. Meta travels with the transaction
permanently and is retrievable via `transaction.get`.

### `create`

`climbing-project` is a new entity — it did not exist in any
previous transaction on this branch. A slug, optional description,
initial properties, and entity-change meta are provided. Property
values are arbitrary JSON (here: a string and an array of strings);
there is no runtime property schema.

### `update`

`alice` is an existing entity. The update writes two new property
values. Her other properties — whatever they are — stay untouched.
The `meta.reasoning` inside this block justifies *this entity
change*, separately from the transaction-level `meta.reasoning`
above.

### `create_managed`

A new `rule` item. The `rule` managed type is declared in
`schema.yaml` and has its own lifecycle; `state: draft` matches
that lifecycle's initial state. Managed items have their own
namespace per type — a `rule` and an entity can share a slug
without conflict.

### `touch`

`bob` is an existing entity that was referenced in this turn but
not changed. The explicit touch marks him as relevant to this
transaction. The next transaction that references Bob will touch
him again, and the touch log accumulates a temporal trail of
attention.

### On commit

Atomic: all of the above lands together, or none of it does. There
is no partial commit.

On success the transaction receives a fresh `tx_id`. After it
commits, on `main` as of this `tx_id`:

- `climbing-project` exists with its three properties.
- `alice` has `city` and `moved_at` as her latest values; her
  earlier properties are unchanged.
- `prefer-absolute-dates` exists as a managed item in state `draft`.
- `entities.touched` returns `climbing-project`, `alice`, and `bob`
  — everyone relevant to this transaction — in reverse
  chronological order.
- A future `checkout` may use this `tx_id` as a branch point to
  explore alternatives without disturbing main.

## Reading the state: `entities.touched`

After the commit, calling `entities.touched` returns the entities
just touched, each with its current state and full provenance.
Assume earlier transactions already created Alice and Bob; this
response shows what comes back right after our transaction lands.

```yaml
command: entities.touched
branch: main
limit: 10
```

Response:

```yaml
- slug: climbing-project
  description: "a side project Alice is starting with Bob"
  properties:
    - property: status
      value: "idea"
      context:
        tx_id: 019537aa-2f1d-7c00-b3f1-0c1d3e4fa9cc   # this commit
        branch: main
        time: "2026-04-22T09:15:42Z"
        reasoning: "Alice described this project in today's message"
    - property: participants
      value: ["alice", "bob"]
      context:
        tx_id: 019537aa-2f1d-7c00-b3f1-0c1d3e4fa9cc   # this commit
        branch: main
        time: "2026-04-22T09:15:42Z"
        reasoning: "Alice described this project in today's message"
  meta: {}
  context:
    tx_id: 019537aa-2f1d-7c00-b3f1-0c1d3e4fa9cc       # this commit
    branch: main
    time: "2026-04-22T09:15:42Z"

- slug: alice
  description: "the user I'm talking to"
  properties:
    - property: name
      value: "Alice"
      context:
        tx_id: 01950001-ab10-7c00-b3f1-0c1d3e4fa9aa   # earlier tx
        branch: main
        time: "2026-03-01T14:00:00Z"
        reasoning: "Alice introduced herself in our first conversation"
    - property: age
      value: 30
      context:
        tx_id: 01950001-ab10-7c00-b3f1-0c1d3e4fa9aa   # earlier tx
        branch: main
        time: "2026-03-01T14:00:00Z"
        reasoning: "Alice introduced herself in our first conversation"
    - property: city
      value: "Berlin"
      context:
        tx_id: 019537aa-2f1d-7c00-b3f1-0c1d3e4fa9cc   # this commit
        branch: main
        time: "2026-04-22T09:15:42Z"
        reasoning: "Alice said she moved last month; current date is 2026-04-22"
    - property: moved_at
      value: "2026-03"
      context:
        tx_id: 019537aa-2f1d-7c00-b3f1-0c1d3e4fa9cc   # this commit
        branch: main
        time: "2026-04-22T09:15:42Z"
        reasoning: "Alice said she moved last month; current date is 2026-04-22"
  meta: {}
  context:
    tx_id: 01950001-ab10-7c00-b3f1-0c1d3e4fa9aa       # entity record (created earlier)
    branch: main
    time: "2026-03-01T14:00:00Z"

- slug: bob
  description: "Alice's friend, mentioned in conversations"
  properties:
    - property: name
      value: "Bob"
      context:
        tx_id: 01950002-de12-7c00-b3f1-0c1d3e4fa9bb   # earlier tx
        branch: main
        time: "2026-03-15T10:30:00Z"
        reasoning: "Alice first mentioned Bob on 2026-03-15"
  meta: {}
  context:
    tx_id: 01950002-de12-7c00-b3f1-0c1d3e4fa9bb       # entity record (created earlier)
    branch: main
    time: "2026-03-15T10:30:00Z"
```

### What to notice

**The entity is a projection, not a stored record.**

Look at `alice`: her `name` and `age` carry context from an earlier
transaction (when she was first introduced), while `city` and
`moved_at` carry context from our latest transaction. A single
entity view mixes assertions from any number of transactions; each
property's own context shows exactly where its current value came
from.

**Provenance is per assertion.**

Every property value has its own `context` block: `tx_id`, `branch`,
`time`, and the `reasoning` that was recorded when that specific
value was asserted. This is not the transaction's overall reasoning;
it is the per-change reasoning, captured at write time.

**The entity's top-level `context` is its record, not its state.**

Each entity also has a top-level `context` — the transaction in
which the entity record itself was created (or most recently
rewritten). It does not carry `reasoning`; reasoning lives on
assertions and on transactions, not on entity records.

**`meta` is empty on snapshot reads.**

The entity's top-level `meta` is `{}`. A snapshot is a projection
across many entity-changes; there is no single entity-change meta
to return. Per-assertion reasoning takes its place on each
property's context.

**Ordering.**

Results are returned most-recently-touched first. `climbing-project`,
`alice`, and `bob` all have their latest touch in this transaction,
so they come back at the top. Entities touched only in earlier
transactions would appear below, with older tx_ids on their latest
touch.

## Contradicting sources

Two sources asserting different values for the same property — where
timestamps alone do not tell you which one is right — is not a
failure mode in terra. It is the point.

Scenario: in April 2026 the agent scrapes Alice's LinkedIn profile
and learns she lives in Berlin. Two days later, in a chat message,
Alice casually says she is in Amsterdam for the month. Both inputs
are recent; neither source is authoritative. The agent commits them
as they come in, each with its own reasoning:

```yaml
# tx 1 — nightly LinkedIn scrape
meta: { reasoning: "nightly LinkedIn profile scrape" }
update:
  - slug: alice
    properties: [{ property: city, value: "Berlin" }]
    meta: { reasoning: "Alice's LinkedIn lists Berlin as current location" }

# tx 2 — chat turn two days later
meta: { reasoning: "chat turn 2026-04-22" }
update:
  - slug: alice
    properties: [{ property: city, value: "Amsterdam" }]
    meta: { reasoning: "Alice mentioned she's 'in Amsterdam for the month' today" }
```

No auto-invalidation. Both assertions live in the store with their
own `tx_id`s and reasoning.

### Snapshot: latest wins

A snapshot read (`entities.touched`, `entities.similar`) returns one
value per property — the latest committed. Alice's `city` comes back
as `Amsterdam`, attributed to the chat transaction:

```yaml
properties:
  - property: city
    value: "Amsterdam"
    context:
      tx_id: 019588cc-0000-7000-0000-000000000002
      branch: main
      time: 2026-04-22T12:00:00Z
      reasoning: "Alice mentioned she's 'in Amsterdam for the month' today"
```

A single clean answer for whoever is reading right now — but note
that "latest" here is an artifact of commit order, not a semantic
judgment. Had the scrape landed after the chat message, Berlin would
have been returned instead. The snapshot does not weigh sources.

### History: both claims, with provenance

`entity.history` with `property: city` returns the full timeline —
each entry carries an entity snapshot as of that transaction, the
properties that changed in it, and the transaction's own meta:

```yaml
- properties:
    - property: city
      value: "Amsterdam"
      context:
        tx_id: 019588cc-0000-7000-0000-000000000002
        reasoning: "Alice mentioned she's 'in Amsterdam for the month' today"
  changed_properties: [city]
  transaction_meta: { reasoning: "chat turn 2026-04-22" }

- properties:
    - property: city
      value: "Berlin"
      context:
        tx_id: 01953568-0000-7000-0000-000000000001
        reasoning: "Alice's LinkedIn lists Berlin as current location"
  changed_properties: [city]
  transaction_meta: { reasoning: "nightly LinkedIn profile scrape" }
```

Both values, both sources, each with two layers of reasoning — the
per-assertion one ("why this specific value was written") and the
per-transaction one ("why this batch of work was committed"). A
caller deciding which to trust can reason about the *sources*
(LinkedIn profile vs. direct chat message) rather than just about
recency — which is precisely the information terra is designed to
preserve. The store does not decide. It keeps the inputs honest.

## Epistemic status

By default every assertion is equal and a snapshot returns the latest
value per property. A project may instead declare **epistemic
statuses** — a small vocabulary marking *how settled* a claim is. A
typical set: `fact`, `hypothesis`, `observation`. This is opt-in, via
the `assertion_statuses` section of `schema.yaml`
([configuration](configuration.md#assertion_statuses)); without it, the
status concept does not exist.

A status is set per entity change, next to `reasoning`, and applies to
every property asserted in that change:

```yaml
update:
  - slug: alice
    status: hypothesis
    properties: [{ property: city, value: "Lyon" }]
    meta: { reasoning: "a guess from her flight bookings" }
```

One status is declared **terminal** (e.g. `fact`) — the consolidating
one. The rest (`hypothesis`, `observation`, ...) are non-terminal
overlays. This changes how a snapshot projects a property:

- The **latest terminal** assertion is the baseline.
- Every **non-terminal** assertion made *after* that baseline is
  layered on top, newest-first, each carrying its own status in its
  property context.
- Everything older than the latest terminal is **consolidated away** —
  a terminal assertion resets the picture. Earlier hypotheses and
  observations remain in `entity.history`, but drop out of the
  snapshot.

So a single property can come back more than once in a snapshot: the
settled `fact` plus the open `hypothesis` and `observation` thrown on
top of it. The reader sees both what is established and what is still
in play, instead of a single latest-wins value.

A property with no terminal assertion yet returns all its overlays —
nothing has consolidated it. A retraction (writing `null`) counts as
terminal: it consolidates the property away, and only later overlays
can re-open it. A write that omits `status` gets the schema's
`default`; status-less assertions written before the schema gained
statuses read as that `default` too.

Statuses layer the same way across branch ancestry: a `fact` inherited
from the parent is the baseline, and a `hypothesis` thrown on a child
branch stacks on top of it without disturbing the parent.
