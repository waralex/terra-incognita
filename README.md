# terra_incognita

An append-only epistemic store: a database of **claims about the world** —
each tied to a transaction and carrying reasoning. Not a database of facts.

Two sources can contradict each other and both remain in the system.
Conflict resolution is the caller's responsibility, not the storage's.

<!-- After pushing to GitHub, add CI badge here:
![CI](https://github.com/<user>/<repo>/actions/workflows/ci.yml/badge.svg)
-->

## Status

**v0.2.** Core model (entities, assertions, branches, managed types,
transaction log, entity history, similarity search) is in place. HTTP
dispatch covers the main commands.

API shape is still settling — this is an active design exploration, not
a production datastore.

## Quickstart

```bash
# 1. Build terra-server
cargo build -p terra-server --release

# 2. Minimal config
mkdir my-terra && cd my-terra
cat > terra-server.yaml <<'EOF'
port: 3000
project_config_path: ./project.yaml
EOF
cat > project.yaml <<'EOF'
data_dir: ./data
schema_path: ./schema.yaml
EOF
cat > schema.yaml <<'EOF'
transaction_meta:
  reasoning: { type: text, required: true }
entity_change_meta:
  reasoning: { type: text, required: true }
branch_meta:
  reasoning: { type: text, required: true }
EOF

# 3. Run
TERRA_SERVER_CONFIG=./terra-server.yaml ../target/release/terra-server

# 4. In another shell — write an assertion
curl -s http://localhost:3000/query \
  -H 'Content-Type: application/json' -d '{
    "command": "transaction",
    "branch": "main",
    "meta": { "reasoning": "first entity" },
    "create": [{
      "slug": "alice",
      "description": "a person I know",
      "properties": [{ "property": "age", "value": 30 }],
      "meta": { "reasoning": "told to me directly" }
    }]
  }'
```

## First target use case

Persistent memory for agents — a code companion, a research assistant, a
tutor — that needs to remember honestly what it heard, from where, and
with what reasoning. Provenance, branching exploration, and coexisting
contradictions are things raw chat history lacks.

## How this relates

If you've seen **Graphiti** or **Datomic**: both care about time, but
both assume the graph should converge toward one truth. Graphiti
auto-invalidates contradicting facts to keep it consistent. Datomic's
bi-temporal model still assumes a single authoritative source.

terra keeps **all** assertions — including contradicting ones — and
hands back the full distribution. Conflict resolution is explicitly the
caller's problem. Uncertainty is data, not a bug to smooth over.

## Core model

### Assertions

An assertion is an atomic claim about one property of one entity,
carried by a transaction, with reasoning. Multiple assertions about the
same property coexist — terra does not auto-resolve.

Uncertainty is expressed in the data: alternative values across
transactions, hedging language in reasoning, differing sources.

### Entities

An entity is addressed by a slug — a human-readable identifier
(unique within the current branch's ancestry). It carries a description
and a set of properties.

Property slugs are free-form — there is no runtime property registry.
The model is open-world: any new property slug is valid, and absence
of an assertion means "we don't know," not "false."

### Transactions

The single mutation primitive. A transaction atomically covers entity
creation, updates, managed-type operations, soft-deletion, and explicit
touches. Transaction metadata is project-defined — fields like
`reasoning`, `question`, `answer` are declared in the data schema
(YAML), not baked into terra.

Each transaction has a `tx_id` that is both a unique identity and a
chronological ordering, so queries like "show me state at tx X" work
naturally. This is also the foundation for future rebase / cherry-pick
semantics.

### Branches

Branches are the unit of isolated exploration, modeled like git. A
child branch inherits everything its parent had at the branch point,
then evolves independently — creating a branch is cheap and does not
duplicate data. The `main` branch is implicit and always present.

`checkout` creates a branch and may carry an embedded initial
transaction in the same atomic step.

### Managed types

Typed, versioned records with an optional lifecycle. Defined in the
project's data schema — tasks, rules, decisions, whatever the caller
wants. Declaring a new managed type in the schema makes it immediately
writable: no code changes, no migration.

The demo client uses a `rule` managed type to persist self-improving
agent instructions across conversations.

### Deletion

Deletion is a new fact. `delete` marks an entity as no-longer-existing
with reasoning, but prior assertions stay queryable — history is
append-only. Re-creating with the same slug later is allowed.

### Embeddings (optional)

When compiled with the `onnx` feature and pointed at an ONNX model
directory (e.g. `all-MiniLM-L6-v2`), terra computes embeddings for
entity descriptions and serves the `entities.similar` query. Without
the feature, similarity queries return empty.

## Invariants

- **Append-only.** Nothing is updated in place.
- **Atomic assertions.** One assertion = one property of one entity.
- **Provenance everywhere.** Every assertion is tied to a transaction
  and its reasoning.
- **Open world.** Absence of an assertion ≠ false. It means we don't
  know.
- **Deletion is a new fact.** "X no longer exists after tx2" does not
  invalidate "X existed at tx1". Both are true. Both stay.
- **Explicit contracts for metadata.** Transaction metadata fields and
  managed types must be declared in YAML before use. Entity properties
  are open — consistency there is the caller's problem.

## Crates

- **terra-core** — domain logic, RocksDB storage, command execution.
  Single library entry point: `Terra::open(path, config, schema, embedder)`
  and `terra.execute(branch, input)`.
- **terra-server** — axum-based HTTP server over terra-core. Single
  endpoint `POST /query` accepting JSON or YAML (selected via
  `Content-Type`).

## HTTP commands

All requests go to `POST /query` with a `command` discriminator and an
optional `branch` (defaults to `main`):

- Writes: `transaction`, `checkout`
- Reads: `transactions.list`, `transaction.get`, `entities.touched`,
  `entities.similar`, `branch.get`, `managed.list`

A `transaction` body accepts: `meta`, `create`, `update`,
`create_managed`, `update_managed`, `delete`, `touch`.

Errors come back with a `kind` field (`validation_error`, `not_found`,
`unknown_command`, `invalid_slug`, `parse_error`, ...).

## Config

Three YAML files:

```yaml
# terra-server.yaml
port: 3000
project_config_path: ./project.yaml
embed_model_dir: ../models/all-MiniLM-L6-v2  # optional, needs `onnx` feature
```

```yaml
# project.yaml
data_dir: ./data
schema_path: ./schema.yaml
```

```yaml
# schema.yaml
transaction_meta:
  reasoning: { type: text, required: true }
  question:  { type: text }
  answer:    { type: text }

entity_change_meta:
  reasoning: { type: text, required: true }

branch_meta:
  reasoning: { type: text, required: true }

managed_types:
  rule:
    fields:
      content:   { type: text, required: true }
      rationale: { type: text }
    lifecycle:
      initial: draft
      states: [draft, active, rejected, promoted]
      visible: [draft, active]
```

## Clients in this repo

- **demo-terra-client** — an exploratory POC used during v0.2
  development to validate terra's shape end-to-end. Wires an LLM
  (OpenAI-compatible or Anthropic) to terra-server as persistent
  memory, with a small web UI. Not a reference integration and not
  maintained as a product — kept here as a working example of the
  shortest path from zero to a talking agent with terra as memory.

See also: [product-docs/concept.md](product-docs/concept.md) for
underlying design rationale.

## Non-goals

- A general-purpose DBMS
- An automatic conflict-resolution / inference engine
- A vector database — embeddings are a convenience, not the primary
  retrieval model
- An event-sourcing framework (similar spirit, different semantics:
  events describe what happened inside the system; assertions are
  claims about the outside world)
- A Datomic clone — terra assumes multiple competing sources of
  partial truth, not one authoritative one

## Stack

- Rust (workspace, edition 2021)
- RocksDB — single storage engine
- axum + tokio — HTTP server
- Optional ONNX Runtime — embedding-backed similarity

Single binary. Zero infrastructure. No Docker, no network dependencies.

## License

Apache-2.0. See [LICENSE](LICENSE).
