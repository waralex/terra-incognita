# terra-incognita

A local, versioned Markdown store for agents, backed by RocksDB. Active development; interfaces and experimental database formats can change.

![CI](https://github.com/waralex/terra-incognita/actions/workflows/ci.yml/badge.svg)

## Current project memory

Markdown is the agent-facing view of a tree of stable-ID blocks. Parsed writes create paragraphs, sections and lists; transactions record author and reason. Per-block revision checks prevent stale edits. History, snapshot links and deletion preserve the record of earlier decisions.

One local service owns the document database and supports multiple chats. Navigation, search, links and history are available through MCP.

- [Enable memory in a project](docs/terra-memory-pilot.md)
- [MCP tools and service operation](docs/engine/document-mcp.md)
- [Agent skill](skills/terra-memory/SKILL.md)
- [Markdown mapping](docs/engine/document-markdown.md) · [Storage architecture](docs/engine/document-store.md)
- [Tests and application layout](markdown/README.md)

## Implementation

`terra-core::documents` stores block versions and ordered trees. `markdown/document-*.mjs` parses and renders Markdown and implements the agent tools. A Rust bridge owns RocksDB; a Node.js MCP proxy connects chats to the shared service. It requires Rust and Node.js; the proxy also runs on Node.js.

History is retained while current-state indexes may be updated in place. This is a local prototype, without replication or automatic conflict resolution. Back up the document database and project registry together; `.local/` is private runtime state and is not committed.


## License

Apache-2.0. See [LICENSE](LICENSE).
