# Working on Terra

Terra is a local, versioned Markdown store for agents. Agents read a bounded Markdown view and edit stable-ID blocks through MCP. The database stores block versions, hierarchy, order and transaction reasons; Markdown parsing belongs to the application layer.

## Where to work

- `crates/terra-core/src/documents/`: document storage, topology, atomic writes and historical reads.
- `crates/terra-core/examples/document-bridge.rs`: local JSONL bridge, owns the database.
- `markdown/document-model.mjs`: Markdown import, export and parsed edits.
- `markdown/document-mcp.mjs`: agent tool contract.
- `markdown/document-memory-server.mjs`: shared authenticated local RPC owner.
- `scripts/terra-document-mcp-proxy.mjs`: per-chat stdio proxy.
- `skills/terra-memory/`: opt-in memory instructions for agents.

Read [the MCP contract](docs/engine/document-mcp.md), [Markdown mapping](docs/engine/document-markdown.md), or [storage design](docs/engine/document-store.md) as needed.

## Contracts to preserve

- UUID identity is independent of title, parent and order.
- A read uses one committed snapshot; depth and node limits must be explicit.
- Mutations validate expected revisions and commit atomically with author/reason. Conflicts require reconsideration, not blind retries.
- Current indexes may change in place; historical versions remain available after replacement or deletion.
- Parse inserted/replacement Markdown into paragraphs, sections and list items. Do not silently replace a container or lose its descendants.
- Keep storage independent of MCP presentation and research workflow semantics.
- One process owns a database. Multiple chats use the shared service. Never use live project memory for tests.

## Verification

From the repository root, select checks relevant to the change:

```sh
RUSTC_WRAPPER= cargo test -p terra-core documents
RUSTC_WRAPPER= cargo build -p terra-core --example document-bridge
npm test --prefix markdown
node markdown/document-integration.mjs
node --test markdown/document-mcp-integration.mjs
```

Document integration tests use temporary databases. `.local/` contains private runtime data and must remain untracked.

## Code Style

- `///` docstrings on all public items (structs, enums, functions,
  methods, traits). This overrides any global instruction to skip
  docstrings.
- Minimal comments otherwise — only where logic isn't self-evident.
- One entity per file. Deep directory structure when needed. No
  god-files, no `utils.rs`, no bags of loosely related things.
- `mod.rs` files contain only `mod` declarations and re-exports —
  no logic, no types, no functions.
- All `impl` blocks for a type live in the file where the type is
  defined. Do not spread `impl` across files unless implementing a
  foreign trait.
- Explicit `use` imports, not `super::` paths.

## Ownership and Concurrency

Assume multi-threaded and async context.

- **No lifetimes on long-lived types.** Structs that persist beyond a
  single function call must not carry lifetime parameters. Use
  `Arc<T>` for shared ownership instead of `&'a T`. Lifetimes are
  acceptable only for short-lived borrows within a function scope
  (iterators, builders, closures).
- **No references for long-term storage.** Storing `&'a T` in a struct
  couples it to the lender's lifetime and makes the type unusable
  across threads and async boundaries. Use `Arc<T>` (or
  `Arc<Mutex<T>>` / `Arc<RwLock<T>>` when interior mutability is
  needed).

## Commit Messages

Short and informative — describe the intent, not the diff. Style from
this repo's log: `<scope>: <lowercase description>`. Do not mention
changes to `CLAUDE.md`, README, or other meta-files in commit messages.
