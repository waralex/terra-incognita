# Query Specification

All queries use verb-target routing over YAML. Sent as POST to `/query` with
`Content-Type: application/yaml`.

## Envelope

```yaml
verb: <action>
target: <resource-type>
```

## Slug Format

- Lowercase ASCII `a-z`, digits `0-9`, hyphens `-`
- No leading/trailing hyphens, no consecutive hyphens `--`
- Cannot be empty

Valid: `military-unit`, `unit-123`, `combat-strength`
Invalid: `Military-Unit`, `unit_name`, `-leading`, `trailing-`, `double--hyphen`

## Value Types

- `string`
- `number`
- `struct` — arbitrary JSON structure, accepts JSON or YAML on input, stored as JSON

---

## Commands

### create entity-type

```yaml
verb: create
target: entity-type
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

### list entity-type

```yaml
verb: list
target: entity-type
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

### get entity-type

Returns entity type with all attached properties.

```yaml
verb: get
target: entity-type
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
    value_type: string
    created_at: "2026-03-07T14:23:10.000000Z"
  - id: 01901234-5678-9abc-def0-444444444444
    slug: troop-count
    value_type: number
    created_at: "2026-03-07T14:23:15.000000Z"
```

### create property

```yaml
verb: create
target: property
slug: combat-strength
value_type: number
description: Offensive capability rating  # optional
```

Response:

```yaml
id: 01901234-5678-9abc-def0-555555555555
slug: combat-strength
value_type: number
description: Offensive capability rating
created_at: "2026-03-07T14:24:30.000000Z"
```

Struct property example:

```yaml
verb: create
target: property
slug: equipment
value_type: struct
description: Equipment and gear details  # optional
```

### list property

All properties:

```yaml
verb: list
target: property
```

Filtered by entity type:

```yaml
verb: list
target: property
entity_type: military-unit
```

Response:

```yaml
- id: 01901234-5678-9abc-def0-333333333333
  slug: unit-name
  value_type: string
  created_at: "2026-03-07T14:23:10.000000Z"
- id: 01901234-5678-9abc-def0-444444444444
  slug: troop-count
  value_type: number
  created_at: "2026-03-07T14:23:15.000000Z"
```

### attach property

Attaches an existing property to an entity type.

```yaml
verb: attach
target: property
entity_type: military-unit
slug: combat-strength
```

Response:

```yaml
status: ok
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
