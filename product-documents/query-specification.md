# Query Specification

## Design Decision: Self-Contained Queries

All queries go to a single endpoint (`POST /query`) and carry their full intent
inside the body. The query document is the complete unit of work — it can be saved
to a file, versioned, shared, and executed later without knowledge of which endpoint
to call or how to construct the URL.

This is a deliberate choice against REST-style routing where intent is split between
HTTP method, URL path, and body. In REST, `POST /entity-types` with `{"slug": "tank"}`
is three pieces of information in three places. Here, one YAML document is enough:

```yaml
command: entity-type.create
slug: tank
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

Valid: `military-unit`, `unit-123`, `combat-strength`
Invalid: `Military-Unit`, `unit_name`, `-leading`, `trailing-`, `double--hyphen`

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
slug: military-unit
description: A collective body of soldiers  # optional
properties: [unit-name, troop-count]         # optional, attach existing properties
```

Batch (all-or-nothing — if any item fails, nothing is created):

```yaml
command: entity-type.create
items:
  - slug: military-unit
    description: A collective body of soldiers
    properties: [unit-name, troop-count]
  - slug: location
```

Provide either `slug` (single) or `items` (batch), not both.

Single response:

```yaml
id: 01901234-5678-9abc-def0-123456789abc
slug: military-unit
description: A collective body of soldiers
created_at: "2026-03-07T14:22:45.123456Z"
```

Batch response:

```yaml
- id: 01901234-5678-9abc-def0-123456789abc
  slug: military-unit
  description: A collective body of soldiers
  created_at: "2026-03-07T14:22:45.123456Z"
- id: 01901234-5678-9abc-def0-234567890abc
  slug: location
  created_at: "2026-03-07T14:22:45.123457Z"
```

### entity-type.list

```yaml
command: entity-type.list
```

Response:

```yaml
- id: 01901234-5678-9abc-def0-111111111111
  slug: location
  description: A geographic area
  created_at: "2026-03-07T14:20:00.000000Z"
- id: 01901234-5678-9abc-def0-222222222222
  slug: military-unit
  created_at: "2026-03-07T14:22:45.123456Z"
```

### entity-type.get

Returns entity type with all attached properties.

```yaml
command: entity-type.get
slug: military-unit
```

Response:

```yaml
id: 01901234-5678-9abc-def0-222222222222
slug: military-unit
description: A collective body of soldiers
created_at: "2026-03-07T14:22:45.123456Z"
properties:
  - id: 01901234-5678-9abc-def0-333333333333
    slug: unit-name
    value_type: struct
    created_at: "2026-03-07T14:23:10.000000Z"
  - id: 01901234-5678-9abc-def0-444444444444
    slug: troop-count
    value_type: range
    created_at: "2026-03-07T14:23:15.000000Z"
```

### property.create

Single:

```yaml
command: property.create
slug: combat-strength
value_type: range
description: Offensive capability rating  # optional
entity_types: [military-unit]              # optional, attach to existing entity types
```

Batch (all-or-nothing — if any item fails, nothing is created):

```yaml
command: property.create
items:
  - slug: unit-name
    value_type: struct
    entity_types: [military-unit]
  - slug: troop-count
    value_type: range
```

Provide either `slug` (single) or `items` (batch), not both.

Single response:

```yaml
id: 01901234-5678-9abc-def0-555555555555
slug: combat-strength
value_type: range
description: Offensive capability rating
created_at: "2026-03-07T14:24:30.000000Z"
```

Batch response:

```yaml
- id: 01901234-5678-9abc-def0-555555555555
  slug: unit-name
  value_type: struct
  created_at: "2026-03-07T14:24:30.000000Z"
- id: 01901234-5678-9abc-def0-666666666666
  slug: troop-count
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
entity_type: military-unit
```

Response:

```yaml
- id: 01901234-5678-9abc-def0-333333333333
  slug: unit-name
  value_type: struct
  created_at: "2026-03-07T14:23:10.000000Z"
- id: 01901234-5678-9abc-def0-444444444444
  slug: troop-count
  value_type: range
  created_at: "2026-03-07T14:23:15.000000Z"
```

### property.attach

Attaches existing properties to entity types.

Single:

```yaml
command: property.attach
entity_type: military-unit
slug: combat-strength
```

Batch (all-or-nothing):

```yaml
command: property.attach
items:
  - entity_type: military-unit
    slug: combat-strength
  - entity_type: military-unit
    slug: unit-name
  - entity_type: location
    slug: unit-name
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

Single:

```yaml
command: entity.create
entity_name: 72nd-mechanized-brigade
entity_type: military-unit  # optional
context:                     # optional
  source: field-report-2026-03-07
```

Batch (all-or-nothing):

```yaml
command: entity.create
items:
  - entity_name: 72nd-mechanized-brigade
    entity_type: military-unit
    context:
      source: field-report-2026-03-07
  - entity_name: kherson-region
    entity_type: location
```

Single response:

```yaml
id: 01901234-5678-9abc-def0-666666666666
entity_id: 01901234-5678-9abc-def0-777777777777
entity_type: military-unit
name: 72nd-mechanized-brigade
timestamp: "2026-03-07T15:00:00.000000+00:00"
```

Batch response:

```yaml
- id: 01901234-5678-9abc-def0-666666666666
  entity_id: 01901234-5678-9abc-def0-777777777777
  entity_type: military-unit
  name: 72nd-mechanized-brigade
  timestamp: "2026-03-07T15:00:00.000000+00:00"
- id: 01901234-5678-9abc-def0-888888888888
  entity_id: 01901234-5678-9abc-def0-999999999999
  entity_type: location
  name: kherson-region
  timestamp: "2026-03-07T15:00:00.000001+00:00"
```

`entity_type` is a set property on the entity, not a schema constraint. If omitted,
the entity is created without a type.

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
  entity_type: military-unit
  name: 72nd-mechanized-brigade
  context:
    source: field-report-2026-03-07
```

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
(hypothesis or refinement). The current membership of the set emerges from the
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
| `property_not_found` | Referenced property not found |
| `reserved_property` | Attempt to create a reserved property |
| `database_error` | Internal database failure |
