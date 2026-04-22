# Configuration

terra is configured via three YAML files:

- `terra-server.yaml` — HTTP server and runtime settings.
- `project.yaml` — data directory and schema location.
- `schema.yaml` — metadata fields and managed types.

The server loads `terra-server.yaml`, which points at `project.yaml`,
which in turn points at `schema.yaml`.

Relative paths inside any of these files are resolved against
**that file's own directory**, not against the process working
directory.

## terra-server.yaml

```yaml
port: 3000                                   # default: 3000
project_config_path: ./project.yaml          # required
embed_model_dir: ./models/all-MiniLM-L6-v2   # optional, requires the `onnx` feature
```

- `port` — TCP port for `POST /query`.
- `project_config_path` — path to `project.yaml`.
- `embed_model_dir` — directory containing `model.onnx` and
  `tokenizer.json`. Required for semantic similarity
  (`entities.similar`). The server must also be built with the
  `onnx` feature.

Lookup order (first existing file wins):

1. `./terra-server.yaml`
2. `./.terra-incognita/terra-server.yaml`
3. path from `$TERRA_SERVER_CONFIG` env var
4. `~/.terra-incognita/terra-server.yaml`

## project.yaml

```yaml
data_dir: ./data              # required
schema_path: ./schema.yaml    # required
max_branch_depth: 8           # default: 8
```

- `data_dir` — RocksDB directory. Created if it does not exist.
- `schema_path` — path to `schema.yaml`.
- `max_branch_depth` — maximum ancestry chain length. A `checkout`
  that would exceed this fails.

## schema.yaml

Declares project-specific contracts: what fields accompany each kind
of metadata, and what managed types exist.

Top-level sections:

```yaml
transaction_meta:     # fields on every transaction
  <name>: <FieldDef>

entity_change_meta:   # fields on every entity create / update
  <name>: <FieldDef>

branch_meta:          # fields on every checkout
  <name>: <FieldDef>

managed_types:
  <type_name>:
    fields:
      <name>: <FieldDef>
    lifecycle:        # optional
      initial: <state>
      states:  [<state>, ...]   # optional; derived if absent
      visible: [<state>, ...]   # optional; empty means no filter
```

All sections are optional and default to empty. A fully empty schema
parses, but no write requires any meta fields unless they are
declared here.

### `FieldDef`

```yaml
<name>:
  type: text          # or: json (default)
  required: false     # default: false
```

- `type` — interpretation hint; not enforced at the storage level.
  - `text` — string.
  - `json` — arbitrary JSON (default when omitted).
- `required` — if `true`, the write fails with `validation_error`
  when this field is missing.

### `transaction_meta`

Validated on every `transaction` write. Example:

```yaml
transaction_meta:
  reasoning: { type: text, required: true }
  question:  { type: text }
  answer:    { type: text }
```

### `entity_change_meta`

Validated on the `meta` block of each `create` / `update` entity
operation inside a transaction.

```yaml
entity_change_meta:
  reasoning: { type: text, required: true }
```

### `branch_meta`

Validated on the top-level `meta` block of `checkout`.

```yaml
branch_meta:
  reasoning: { type: text, required: true }
```

### `managed_types`

Each entry declares one managed type.

```yaml
managed_types:
  rule:
    fields:
      content:   { type: text, required: true }
      rationale: { type: text }
    lifecycle:
      initial: draft
      states:  [draft, active, rejected, promoted]
      visible: [draft, active]
```

- `fields` — declarations for the managed item body. Same `FieldDef`
  shape as above.
- `lifecycle` (optional) — a state machine for this type:
  - `initial` (required) — state assigned on create. Must appear in
    `states`.
  - `states` (optional) — the full set of valid states. If omitted,
    derived as `{initial} ∪ visible`.
  - `visible` (optional) — states returned by `managed.list`. Empty
    means no filter (every state is listed). Every value must
    appear in `states`.

Constraint: if a `lifecycle` is declared, the type's `fields` must
not include a field named `state` — it is reserved for the
lifecycle state.
