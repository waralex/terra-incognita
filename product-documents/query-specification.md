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

```yaml
command: entity-type.create
slug: military-unit
description: A collective body of soldiers  # optional
```

Response:

```yaml
id: 01901234-5678-9abc-def0-123456789abc
slug: military-unit
description: A collective body of soldiers
created_at: "2026-03-07T14:22:45.123456Z"
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

```yaml
command: property.create
slug: combat-strength
value_type: range
description: Offensive capability rating  # optional
```

Response:

```yaml
id: 01901234-5678-9abc-def0-555555555555
slug: combat-strength
value_type: range
description: Offensive capability rating
created_at: "2026-03-07T14:24:30.000000Z"
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

Attaches an existing property to an entity type.

```yaml
command: property.attach
entity_type: military-unit
slug: combat-strength
```

Response:

```yaml
status: ok
```

### entity.create

```yaml
command: entity.create
entity_type: military-unit
name: 72nd Mechanized Brigade
kind: hypothesis  # optional, default: hypothesis
context:          # optional
  source: field-report-2026-03-07
```

Response:

```yaml
id: 01901234-5678-9abc-def0-666666666666
entity_id: 01901234-5678-9abc-def0-777777777777
entity_type: military-unit
name: 72nd Mechanized Brigade
kind: hypothesis
timestamp: "2026-03-07T15:00:00.000000+00:00"
```

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
| `database_error` | Internal database failure |
