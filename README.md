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

terra keeps **all** assertions — including contradicting ones. The
default snapshot read returns latest-wins, and `entity.history`
replays the full timeline of claims with their sources. Conflict
resolution is the caller's problem, and the per-assertion reasoning
lets you reason about *sources* rather than just recency.

## What's stored

terra is built around two kinds of objects:

- **Entities** — open records identified by a slug, carrying any
  set of properties. No entity types, no runtime property registry:
  any property slug is valid, values are arbitrary JSON.
- **Managed items** — typed records declared in `schema.yaml`, each
  type with its own fields and an optional state lifecycle. Good
  for tasks, rules, decisions — anything with known structure.

Both live on **branches** — git-like isolated lines of history —
and all writes go through **transactions**, atomic batches that
carry their own metadata and a time-ordered `tx_id`. Nothing is
ever overwritten: the current value of anything is a projection
over the history of transactions, and the state at any past
transaction can be reconstructed.

## Documentation

- [Concepts](docs/concepts.md) — data model, branches, touching,
  deletion, with a worked transaction example.
- [HTTP API](docs/api.md) — command reference for `POST /query`.
- [Configuration](docs/configuration.md) — `terra-server.yaml`,
  `project.yaml`, `schema.yaml` reference.
- [Embedded usage](docs/embedded.md) — using `terra-core` as a Rust
  library, without the HTTP server.

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

## Clients in this repo

- **[demo-terra-client](demo-terra-client/)** — an exploratory POC
  used during v0.2 development to validate terra's shape end-to-end.
  Wires an LLM (Anthropic or OpenAI) to terra-server as persistent
  memory, with a small web UI. Not a reference integration and not
  maintained as a product — kept here as a working example of the
  shortest path from zero to a talking agent with terra as memory.
  See [demo-terra-client/README.md](demo-terra-client/README.md) for
  setup and environment variables.

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
