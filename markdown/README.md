# Markdown applications

The current application serves versioned Markdown memory through MCP. Start with [project setup](../docs/terra-memory-pilot.md), [MCP tools](../docs/engine/document-mcp.md), and the [agent skill](../skills/terra-memory/SKILL.md).

## Current implementation

`document-model.mjs` maps Markdown to blocks; `document-mcp.mjs` exposes agent operations; `document-memory-server.mjs` owns the database for shared MCP access. `document-bridge.mjs` talks to the Rust document store.

See [Markdown mapping](../docs/engine/document-markdown.md) and [storage architecture](../docs/engine/document-store.md) for contracts and limits.

## Verification

From the repository root:

```sh
npm ci --prefix markdown
RUSTC_WRAPPER= cargo build -p terra-core --example document-bridge
npm test --prefix markdown
node markdown/document-integration.mjs
node --test markdown/document-mcp-integration.mjs
node --test scripts/*.test.mjs
```

Real-database document tests use temporary databases. [Agent experiments](experiments/README.md) preserve separate usability trials.
