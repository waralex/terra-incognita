# Query Specification

## Design Decision: Self-Contained Queries

All queries go to a single endpoint (`POST /query`) and carry their full intent
inside the body. The query document is the complete unit of work — it can be saved
to a file, versioned, shared, and executed later without knowledge of which endpoint
to call or how to construct the URL.

This is a deliberate choice against REST-style routing where intent is split between
HTTP method, URL path, and body. In REST, `POST /entity-types` with `{"slug": "book"}`
is three pieces of information in three places. Here, one YAML document is enough:

```yaml
command: entity-type.create
slug: book
```

Consequences:
- One endpoint, one content type, one format
- Queries are portable — copy a file, send it anywhere
- Tooling is trivial — `curl -d @query.yml` works for everything
- No URL construction, no path parameters, no method selection

## Batch Convention

All mutating commands (`*.create`, `*.attach`) support batch input via `items`.
The contract:

- **Single mode:** fields at the top level (`slug`, `entity_name`, etc.)
- **Batch mode:** an `items` array, each element carrying the same fields
- **Mutually exclusive:** provide either top-level fields or `items`, not both
- **All-or-nothing:** batch operations are atomic — if any item fails, nothing is committed
- **Response shape matches input:** single mode returns an object, batch mode returns an array
- **Backward compatible:** existing single-format queries work unchanged

Read-only commands (`*.list`, `*.get`, `log.list`) do not need batch input.

## Envelope

```yaml
command: <command-name>
```

The `command` field determines the operation. All other fields are command-specific
parameters. Unknown fields are ignored.

## Slug Format

- Lowercase ASCII `a-z`, digits `0-9`, hyphens `-`
- No leading/trailing hyphens, no consecutive hyphens `--`
- Cannot be empty

Valid: `research-project`, `unit-123`, `page-count`
Invalid: `Research-Project`, `unit_name`, `-leading`, `trailing-`, `double--hyphen`

## Value Types

- `set` — classification/tagging via membership assertions (contains / not_contains)
- `struct` — arbitrary JSON structure; a single string, number, or boolean is a trivial case
- `range` — interval (numbers, dates, etc.); a single value is a degenerate range (start == end). Interpretation type is specified at query time, not at property creation

---

## Commands

### entity-type.create

Single:

```yaml
command: entity-type.create
slug: book
description: A published written work  # optional
properties: [title, page-count]         # optional, attach existing properties
```

Batch (all-or-nothing — if any item fails, nothing is created):

```yaml
command: entity-type.create
items:
  - slug: book
    description: A published written work
    properties: [title, page-count]
  - slug: author
```

Provide either `slug` (single) or `items` (batch), not both.

Single response:

```yaml
id: 01901234-5678-9abc-def0-123456789abc
slug: book
description: A published written work
created_at: "2026-03-07T14:22:45.123456Z"
```

Batch response:

```yaml
- id: 01901234-5678-9abc-def0-123456789abc
  slug: book
  description: A published written work
  created_at: "2026-03-07T14:22:45.123456Z"
- id: 01901234-5678-9abc-def0-234567890abc
  slug: author
  created_at: "2026-03-07T14:22:45.123457Z"
```

### entity-type.list

```yaml
command: entity-type.list
```

Response:

```yaml
- id: 01901234-5678-9abc-def0-111111111111
  slug: author
  description: A person who writes books
  created_at: "2026-03-07T14:20:00.000000Z"
- id: 01901234-5678-9abc-def0-222222222222
  slug: book
  created_at: "2026-03-07T14:22:45.123456Z"
```

### entity-type.get

Returns entity type with all attached properties.

```yaml
command: entity-type.get
slug: book
```

Response:

```yaml
id: 01901234-5678-9abc-def0-222222222222
slug: book
description: A published written work
created_at: "2026-03-07T14:22:45.123456Z"
properties:
  - id: 01901234-5678-9abc-def0-333333333333
    slug: title
    value_type: struct
    created_at: "2026-03-07T14:23:10.000000Z"
  - id: 01901234-5678-9abc-def0-444444444444
    slug: page-count
    value_type: range
    created_at: "2026-03-07T14:23:15.000000Z"
```

### property.create

Single:

```yaml
command: property.create
slug: rating
value_type: range
description: Average reader rating  # optional
entity_types: [book]                 # optional, attach to existing entity types
```

Batch (all-or-nothing — if any item fails, nothing is created):

```yaml
command: property.create
items:
  - slug: title
    value_type: struct
    entity_types: [book]
  - slug: page-count
    value_type: range
```

Provide either `slug` (single) or `items` (batch), not both.

Single response:

```yaml
id: 01901234-5678-9abc-def0-555555555555
slug: rating
value_type: range
description: Average reader rating
created_at: "2026-03-07T14:24:30.000000Z"
```

Batch response:

```yaml
- id: 01901234-5678-9abc-def0-555555555555
  slug: title
  value_type: struct
  created_at: "2026-03-07T14:24:30.000000Z"
- id: 01901234-5678-9abc-def0-666666666666
  slug: page-count
  value_type: range
  created_at: "2026-03-07T14:24:30.000001Z"
```

### property.list

All properties:

```yaml
command: property.list
```

Filtered by entity type:

```yaml
command: property.list
entity_type: book
```

Response:

```yaml
- id: 01901234-5678-9abc-def0-333333333333
  slug: title
  value_type: struct
  created_at: "2026-03-07T14:23:10.000000Z"
- id: 01901234-5678-9abc-def0-444444444444
  slug: page-count
  value_type: range
  created_at: "2026-03-07T14:23:15.000000Z"
```

### property.attach

Attaches existing properties to entity types.

Single:

```yaml
command: property.attach
entity_type: book
slug: rating
```

Batch (all-or-nothing):

```yaml
command: property.attach
items:
  - entity_type: book
    slug: rating
  - entity_type: book
    slug: title
  - entity_type: author
    slug: title
```

Response (single):

```yaml
status: ok
```

Response (batch):

```yaml
status: ok
count: 3
```

### entity.create

Creates a new entity and optionally asserts facts and hypotheses in one transaction.
Everything is atomic — if validation fails, the entity is not created.

```yaml
command: entity.create
entity: brave-new-world
description: A dystopian novel by Aldous Huxley  # optional
reasoning: initial catalog import                 # optional, transaction-level reasoning
facts:                                            # optional
  - entity_type: book
    properties:
      page-count: {eq: 311}
      title: A Brave New World
    reasoning: from publisher metadata
hypotheses:                                       # optional
  - entity_type: book
    properties:
      rating: {from: 4.0, to: 4.5}
    reasoning: estimated from similar titles
```

Each fact and hypothesis carries:
- `entity_type` — which entity type's properties are being asserted
- `properties` — property slug → typed value (see Property Value Formats below)
- `reasoning` — per-assertion reasoning (why this specific value)

**Fact uniqueness constraint:** within a single transaction, two facts cannot assert
the same property on the same entity type. If values are uncertain, express them as
hypotheses instead. Violating this returns a `conflicting_facts` error.

Response:

```yaml
tx_id: 01901234-5678-9abc-def0-aaaaaaaaaaaa
facts:
  - id: 01901234-5678-9abc-def0-bbbbbbbbbbbb
    timestamp: "2026-03-07T15:00:00.000000+00:00"
    entity_id: 01901234-5678-9abc-def0-777777777777
    tx_id: 01901234-5678-9abc-def0-aaaaaaaaaaaa
    properties: {page-count: {eq: 311}, title: A Brave New World}
    reasoning: from publisher metadata
hypotheses:
  - id: 01901234-5678-9abc-def0-cccccccccccc
    timestamp: "2026-03-07T15:00:00.000001+00:00"
    entity_id: 01901234-5678-9abc-def0-777777777777
    tx_id: 01901234-5678-9abc-def0-aaaaaaaaaaaa
    properties: {rating: {from: 4.0, to: 4.5}}
    reasoning: estimated from similar titles
```

### entity.assert

Asserts facts and hypotheses about an existing entity. Same structure as `entity.create`
but the entity must already exist (no `description` field).

```yaml
command: entity.assert
entity: brave-new-world
reasoning: updated analysis after second review
facts:
  - entity_type: book
    properties:
      rating: {eq: 4.2}
    reasoning: confirmed by aggregated reviews
hypotheses:
  - entity_type: book
    properties:
      page-count: {eq: 288}
    reasoning: different edition may have fewer pages
```

Response: same shape as `entity.create`.

### entity.list

Lists all active entities.

```yaml
command: entity.list
```

Response:

```yaml
- id: 01901234-5678-9abc-def0-777777777777
  slug: brave-new-world
- id: 01901234-5678-9abc-def0-999999999999
  slug: aldous-huxley
```

### entity.get

Returns an entity projected onto an entity type. For each property attached to the
entity type, shows the latest fact value (or `unknown` if no fact exists) and the
number of pending hypotheses recorded after the latest fact.

```yaml
command: entity.get
entity: brave-new-world
entity_type: book
```

Response:

```yaml
entity_id: 01901234-5678-9abc-def0-777777777777
entity_slug: brave-new-world
entity_type: book
properties:
  - slug: title
    value_type: struct
    value: A Brave New World
    known: true
    pending: 0
  - slug: page-count
    value_type: range
    value: {eq: 311}
    known: true
    pending: 1
  - slug: rating
    value_type: range
    value: {eq: 4.2}
    known: true
    pending: 0
```

If no fact has been recorded for a property:

```yaml
  - slug: genre
    value_type: set
    value: null
    known: false
    pending: 3
```

### log.list

Returns all assertion log entries in reverse chronological order.

```yaml
command: log.list
```

Response:

```yaml
- id: 01901234-5678-9abc-def0-666666666666
  timestamp: "2026-03-07T15:00:00.000000+00:00"
  entity_id: 01901234-5678-9abc-def0-777777777777
  entity_type: book
  name: brave-new-world
  context:
    source: catalog-import-2026-03-07
```

---

## Property Value Formats

Property values in `facts` and `hypotheses` arrays are typed according to the
property's `value_type`:

**Set** — membership assertions:

```yaml
certification: {contains: [gold, platinum]}
tags: {contains: [fiction], not_contains: [non-fiction]}
```

**Range** — numeric/ordinal interval or exact value:

```yaml
bpm: {eq: 128}                    # exact value
page-count: {from: 200, to: 400}  # closed range
rating: {from: 3.5}               # open-ended (>= 3.5)
year: {to: 2020}                   # open-ended (<= 2020)
```

**Struct** — arbitrary JSON structure (passthrough):

```yaml
title: A Brave New World           # string shorthand
metadata: {genre: fiction, lang: en}
```

Any value that doesn't match set or range markers is treated as struct.

---

## Reserved Properties

The following property slugs are reserved and cannot be created via `property.create`:

- `entity-uuid` — the unique identifier of the entity
- `entity-name` — the human-readable name (slug) of the entity
- `entity-type` — the type classification of the entity

These properties are built-in and managed by the system.

---

## Set Semantics

A `set` property models classification and tagging. Each assertion on a set property
is an atomic membership claim — `contains` or `not_contains` — with a single value
or a vector of values.

Each claim is an independent assertion with its own confidence, source, and kind
(hypothesis or fact). The current membership of the set emerges from the
collection of assertions.

Example — classifying an entity:

```yaml
# hypothesis: it's a bee
op: contains
value: bee

# hypothesis: it's an insect and a living organism
op: contains
values: [insect, living-organism]

# hypothesis: it's not a rock
op: not_contains
value: rock
```

---

## Errors

```yaml
error:
  kind: <error-type>
  message: <description>
```

Error kinds:

| Kind | Meaning |
|------|---------|
| `parse_error` | Invalid YAML or missing required fields |
| `invalid_slug` | Slug format violation |
| `duplicate_entity_type` | Entity type already exists |
| `duplicate_property` | Property already exists |
| `entity_type_not_found` | Referenced entity type not found |
| `property_not_found` | Referenced property not found or not attached to entity type |
| `reserved_property` | Attempt to create a reserved property |
| `entity_not_found` | Entity not found by slug |
| `entity_already_exists` | Entity with this slug already exists (during `entity.create`) |
| `conflicting_facts` | Two facts in the same transaction assert the same property on the same entity type |
| `assertion_error` | Type mismatch or other assertion validation failure |
| `storage_error` | Internal storage failure |
| `database_error` | Internal database failure |
