# Agent Markdown storage

Terra stores an ordered tree of versioned blocks and returns Markdown views to agents. Start with the tool contract; storage details are only needed when changing the engine.

- [Project setup](terra-memory-pilot.md): connect a chat to shared project memory.
- [MCP contract](engine/document-mcp.md): bounded reads, search, parsed writes, deletion, history and links.
- [Markdown model](engine/document-markdown.md): how headings, paragraphs and lists become blocks.
- [Storage](engine/document-store.md): identity, transactions, indexes and historical reads.
- [Agent skill](../skills/terra-memory/SKILL.md): everyday usage and when to retain knowledge.
- [Verification](../markdown/README.md): tests and application layout.

The engine retains versions and reasons; it does not decide whether a statement is true. Task states, hypotheses and research scheduling are application concerns, not required memory protocols.
