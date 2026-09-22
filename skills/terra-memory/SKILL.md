---
name: terra-memory
disable-model-invocation: true
description: Use Terra project memory during ordinary development to retrieve decisions, constraints and practical knowledge, and update them when work changes what is true. Applies when the terra-memory MCP is connected.
---

# Project memory

This explicit invocation enables Terra project memory for the current conversation. Open the project root with read(), then continue the current task; do not inventory or migrate the whole project. If the user asks to stop using Terra, stop reading/writing it for this conversation.

Use the connected terra-memory tools. If unavailable, say so; do not claim knowledge was saved.

Search for the entities relevant to the task. Open the matching sections, following child links only when needed. On an empty project, start with the work at hand rather than mapping the entire repository. Memory is recorded context, not proof that code still behaves that way. Verify consequential claims against current work.

Save discoveries worth reusing: a surprising constraint, a decision and its reason, a reliable verification recipe, or a pitfall. Keep the current statement concise. Mention the source and scope when needed (branch-only experiment, user preference, unverified hypothesis). Avoid transcripts, code copies, progress diaries and routine successes.

Project memory is one tree. Start with read() without an id, then open child IDs as needed. Use read(depth:63) to retrieve a whole subtree in one call when needed; truncated indicates omitted descendants. Search reports is_empty and scanned_blocks, so zero matches alone does not establish whether memory exists. Search before creating another topic. Use write(parent, markdown, reason) to add a topic inside a section; paragraphs, headings and lists are parsed automatically. Use write(target, expected, markdown, reason) to replace a Text block. Copy expected from its read revision. Container replacement is intentionally unsupported; do not rewrite a whole section to change one paragraph.

write returns written entries with IDs, parent IDs and revisions for every inserted block; reuse them instead of rereading just to discover addresses.

Remove obsolete current content with remove(id, expected, reason). For a section or list with children, first read it completely and pass subtree_revision too. Deletion removes the subtree from current reads and searches while retaining version history. A changed descendant rejects the whole removal; reread and reconsider. The project root cannot be deleted.

Use change(edits, reason) for related section-title and checkbox changes in one transaction. Each edit includes id and expected, plus title or state (Todo/Done). Keep the reason short and concrete. A stale revision means reread and reconsider, never blindly retry. Read Markdown and tool descriptions for exact argument names.

When a conclusion changes, update its current statement. history(id) retains earlier versions and reasons, and lists changed_blocks from the same transaction: inspect those to recover added siblings. Read with at to inspect an old snapshot. History is per block, not automatically the history of an entire subtree. Use it when rationale matters, not on every read.


For a project using Terra as primary memory, write project knowledge here instead of maintaining a duplicate file-memory version. Existing file memory remains a legacy source until reconciled, not automatically authoritative. Do not delete it or disable the host's memory globally. User requests take precedence over remembered decisions.

Use `link_templates` from Markdown reads (replace BLOCK_ID), or `href` /
`snapshot_href` from JSON reads, for live / pinned references. Insert links using write; do not
invent legacy doc/path addresses. `links(id)` checks outgoing links, and
`backlinks(id)` finds current source blocks linking to that ID within this project.
Pinned targets keep their snapshot after deletion; unpinned deleted targets are
reported as deleted. External and cross-project targets are not validated.

`read` defaults to Markdown only: each block has an HTML metadata comment with
its ID, revision, kind, parent and optional checkbox state. Use that revision as
`expected`; no extra read is required to convert formats. Request `format: "json"`
only when structured blocks are useful; the two formats are mutually exclusive.
`link_templates` supplies addresses once per Markdown response: replace BLOCK_ID
with the chosen block ID. In JSON mode blocks retain href/snapshot_href.

Root reads include `entrypoints`: selected links from anywhere in the tree, without loading their bodies into your context. Open relevant ones as needed. Set a concise “when to read this” label with `change(edits:[{id,expected,entrypoint:"..."}],reason)`; set `entrypoint:null` to remove it. Keep this collection small and useful across sessions, not a list of everything. This changes neither hierarchy nor the target text; labels and removals retain history.
