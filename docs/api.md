# HTTP API

terra-server has one endpoint — `POST /query` — that accepts a
command envelope as JSON or YAML. This file is the shape reference.
For concepts (entity vs managed, branches, touching, etc.) see
[concepts.md](concepts.md).

## Envelope

```yaml
command: <name>       # required
branch: <slug>        # optional, default: main
<command-specific fields flattened at the top level>
```

Content-Type: `application/json` or `application/x-yaml` (default).

Every non-2xx response has the shape:

```yaml
error: <message>
kind: <stable kind>
```

Error kinds: `parse_error`, `invalid_slug`, `validation_error`,
`unknown_command`, `not_found`, `conflict`, `storage_error`,
`serialize_error`.

## transaction

Atomic batch mutation.

Request:

```yaml
command: transaction
branch: main
meta: { reasoning: "..." }      # per transaction_meta in schema.yaml
create:         [<Entity>]      # optional
update:         [<Entity>]      # optional
create_managed: [<Managed>]     # optional
update_managed: [<Managed>]     # optional
delete:         [<Delete>]      # optional
touch:          [<Touch>]       # optional
```

Types:

```yaml
# Entity
slug: alice
description: "..."              # optional, any JSON
status: <status>                # optional; per assertion_statuses in schema.yaml
properties:
  - { property: <slug>, value: <any JSON> }
meta: { reasoning: "..." }      # per entity_change_meta in schema.yaml

# Managed
type_name: <slug>               # must be declared in managed_types
slug: <slug>                    # unique within (type_name, branch ancestry)
state: <state>                  # required on create (must be lifecycle.initial); optional on update
fields: { <field>: <value>, ... }

# Delete
entity: <slug>
reasoning: <any JSON>

# Touch
entity: <slug>
reasoning: "..."                # string
```

Response:

```yaml
meta: { ... }                   # echoed from the request
context:
  tx_id: 019537aa-...
  branch: main
  time: 2026-04-22T09:15:42Z
```

To see what the transaction actually created / updated / deleted /
touched, use `transaction.get`.

## checkout

Create a new branch, atomically committing a first transaction on it.

Request:

```yaml
command: checkout
branch: main                    # parent branch
slug: feature-x                 # new branch slug
meta: { reasoning: "..." }      # per branch_meta
created_from_tx: <uuid>         # optional; defaults to parent's latest tx
transaction:                    # required; same shape as `transaction` body
  meta: { reasoning: "..." }
  create: [...]
  ...
```

Response:

```yaml
branch: feature-x
created_from_tx: <uuid>
transaction:
  meta: { ... }
  context: { tx_id, branch: feature-x, time }
```

## transactions.list

Transactions committed on the branch, newest first.

Request:

```yaml
command: transactions.list
branch: main
at_tx: <uuid>                   # optional upper bound (inclusive)
limit: 50                       # default 50
```

Response:

```yaml
- meta: { ... }
  context: { tx_id, branch, time }
- ...
```

## transaction.get

Full detail of one transaction — what it created / updated / deleted /
touched. Cross-branch: `tx_id` may refer to a transaction on any
branch regardless of the `branch` field.

Request:

```yaml
command: transaction.get
branch: main
tx_id: <uuid>                   # optional; defaults to the latest on the branch
```

Response:

```yaml
meta: { ... }
branch: main
context: { tx_id, branch, time }
created:          [<Entity>]
updated:          [<Entity>]
deleted:
  - slug: <slug>
    reasoning: <json>
    meta: { ... }
    context: { ... }
touched:
  - slug: <slug>
    reasoning: "..."
created_managed:  [<Managed>]   # omitted if empty
updated_managed:  [<Managed>]   # omitted if empty
```

## entities.touched

Entities most recently touched on the branch, newest first.

Request:

```yaml
command: entities.touched
branch: main
at_tx: <uuid>                   # optional upper bound
limit: 50                       # default 50
```

Response: array of `<Entity>` (see below).

## entities.similar

Semantic similarity search over entity descriptions. Requires the
`onnx` feature and a configured `embed_model_dir`; otherwise returns
an empty array.

Request:

```yaml
command: entities.similar
branch: main
queries:
  - "a person living in Berlin"
limit: 50                       # default 50
min_similarity: 0.5             # default 0.0
at_tx: <uuid>                   # optional upper bound
```

Response:

```yaml
- <Entity fields>
  similarity: 0.82
  matched_query: 0              # index into the `queries` array
```

## entities.grep

Regex search over entities on the branch, newest first. The pattern is
matched against the fields listed in `in`; a match in any of them
includes the whole entity.

The regex flavor is Rust's `regex` crate (linear-time, no
backtracking). It is case-sensitive — prefix with `(?i)` to ignore
case. For `value`, string values are matched against their raw text and
all other JSON values against their compact serialization (e.g. `42`,
`true`, `{"k":"v"}`).

Request:

```yaml
command: entities.grep
branch: main
pattern: "^auth-"               # required, regex
in: [slug]                      # any of: slug, property, value, reasoning; default [slug]
properties: true                # include properties in the result; default true
at_tx: <uuid>                   # optional upper bound
limit: 50                       # default 50
```

Response: array of `<Entity>` (see below). With `properties: false`
each entity carries only its slug, description, and provenance — the
`properties` array is empty.

Errors: `validation_error` for an invalid regex; `parse_error` for an
unknown field name in `in`.

## entity.history

Change history of a single entity — snapshots at every transaction
that touched it, newest first.

Two pagination modes:

- **Cursor** (default) — `at_tx` is the upper bound (inclusive),
  `limit` is the page size.
- **Range** — set `tx_id_from` (inclusive lower bound). Upper bound
  is `tx_id_to` if present, otherwise `at_tx`, otherwise the branch
  head. `limit` still caps the result.

Request:

```yaml
command: entity.history
branch: main
entity: alice                   # required
property: age                   # optional; only txs that touched this property
at_tx: <uuid>                   # optional upper bound
limit: 50                       # default 50
tx_id_from: <uuid>              # optional; switches to range mode
tx_id_to: <uuid>                # optional upper bound for range mode
```

Response:

```yaml
- <Entity fields>                     # entity snapshot at this tx
  changed_properties: [age, city]     # property slugs changed in this tx
  transaction_meta:                   # per `transaction_meta` schema
    reasoning: "..."
- <Entity fields>
  changed_properties: [...]
  transaction_meta: { ... }
```

Errors: `not_found` if the entity does not exist on the branch.

## branch.get

Metadata of a branch.

Request:

```yaml
command: branch.get
branch: feature-x
```

Response:

```yaml
slug: feature-x
parent: main
meta: { ... }
context: { tx_id, branch, time }   # of the checkout
```

For implicit `main`: `parent: main`, `context.tx_id` is the nil UUID.

## managed.list

All managed items on the branch, filtered by each type's `visible`
lifecycle states.

Request:

```yaml
command: managed.list
branch: main
at_tx: <uuid>                   # optional upper bound
```

Response: array of `<Managed>` entries with a `context`.

## Response shape: `<Entity>`

```yaml
slug: <slug>
description: <json>              # optional
properties:
  - property: <slug>
    value: <any JSON>
    context:
      tx_id: ...
      branch: main
      time: ...
      reasoning: "..."           # per-assertion reasoning
      status: <status>           # per assertion_statuses; omitted when not configured
meta: { ... }                    # empty on snapshot reads; populated on transaction.get
context:
  tx_id: ...                     # of the entity record
  branch: main
  time: ...
```

`context.reasoning` appears only on property contexts. On entity,
transaction, branch, and managed contexts it is omitted. `context.status`
likewise appears only on property contexts, and only when the project
declares `assertion_statuses`. When statuses are configured, a single
property may appear more than once — the consolidating (terminal)
value plus later non-terminal overlays; see
[concepts.md](concepts.md#epistemic-status).

## Response shape: `<Managed>`

```yaml
type_name: <slug>
slug: <slug>
state: <state>                   # omitted if the type has no lifecycle
fields: { ... }
context: { tx_id, branch, time }
```
