# Document-memory MCP

Current project memory uses stable block IDs and per-block revisions. One shared local service owns RocksDB; chat clients connect through `scripts/terra-document-mcp-proxy.mjs`. See [project setup](../terra-memory-pilot.md) and the [agent skill](../../skills/terra-memory/SKILL.md).

## Tools

| Tool | Purpose |
| --- | --- |
| `read` | Open root or block; optional `depth` (0–63, default 1), `at` snapshot, `format` (`markdown` default or `json`). |
| `search` | Search current Markdown-stripped titles and text; not history. `mode: "literal"` (default) or `"regex"` (case-insensitive Unicode JavaScript pattern, without `/…/` delimiters). Reports `is_empty`, `scanned_blocks`, `complete`, and hit truncation. |
| `write` | Parse Markdown: append under `parent`, insert beside `anchor` with `expected` and `side`, or replace a Text `target` with `expected`. |
| `change` | Atomically rename sections / toggle checkboxes: 1–30 edits containing `id`, `expected`, and `title` or `state` (`Todo`/`Done`). |
| `remove` | Tombstone a block/subtree with `id`, `expected`, `reason`; descendants require `subtree_revision` from a complete current read. Root deletion is refused. |
| `history` | Per-block versions with author, reason and `changed_blocks`; `before` is inclusive. |
| `links` | Outgoing links from one block, optionally at a snapshot. |
| `backlinks` | Current sources referring to a target in the same project, including pinned references. |

All mutations require a reason. Copy `expected` from a read or write receipt; on conflict, reread and reconsider. Containers cannot be replaced implicitly. Multiple paragraphs become independent blocks. `write.written` returns IDs, revisions and parents of all inserted/replaced blocks; `blocks` lists top-level IDs.

## Reading and history

Markdown responses contain each visible text once, with HTML comments carrying ID, revision, kind, parent and optional checkbox state. JSON responses contain structured `blocks` instead of Markdown. Snapshot and completeness metadata are shared across formats. List-item previews appear only when their text child is outside the visible depth.

Reads use one committed snapshot. `truncated` reports omitted descendants; complete current reads include `subtree_revision` for safe subtree deletion. Limits: 10000 nodes per read/search, 50 search hits, 1000 removed blocks. Search's `complete` concerns tree coverage; its `truncated` concerns matching hits.

`history` is not a synthesized history of an entire section. Follow `changed_blocks` to inspect related edits, or read with `at` to recover old context. Deletion hides current content but preserves history.

## Links

Canonical addresses: `/?root=ROOT_UUID&id=BLOCK_UUID[&at=TX_UUID]`.
Markdown reads provide `link_templates` once: replace `BLOCK_ID` with the desired ID. JSON blocks provide `href` and `snapshot_href`. The pinned address uses the read snapshot, not just the block's last edit. Write links as ordinary Markdown; renaming a block preserves its address.

Inline and reference-style links are derived from text on demand, with definitions resolved across the project. Target statuses: `ok`, `deleted`, `missing`, `outside_scope`, `external`, `legacy`, `invalid`. No external requests or cross-project validation occur. Unpinned targets use the source snapshot; pinned targets use their explicit `at`.

There is **no persistent link index** in this adapter. Each query scans a bounded project snapshot (10000 blocks, up to 200 matching edges), reporting completeness/truncation.

## Service operation

From the repository root, with an existing registry:

```sh
RUSTC_WRAPPER= cargo build -p terra-core --example document-bridge
node markdown/document-memory-server.mjs .local/document-memory
```

The registry (`registry.json`, mode 0600) holds credentials and project root IDs; `db/` holds RocksDB. Back up both. Do not start another owner of the same database. RPC binds to loopback port 8097 with bearer authentication (override with `MEMORY_RPC_PORT`). An optional separate read-only browser is available via `node markdown/document-viewer.mjs` on loopback port 8096; see [project setup](../terra-memory-pilot.md#read-only-browser).

The existing local launchd service is `local.terra.document-memory`:

```sh
launchctl kickstart -k gui/$(id -u)/local.terra.document-memory
```

Its plist/logs live under `.local/document-memory/`; the plist is not installed as a login item. Re-bootstrap it after login if needed. Reconnect chat MCP clients after tool-schema changes.

For isolated experiments only, `node markdown/document-mcp.mjs DB ROOT_UUID BRIDGE` runs a standalone stdio owner; use absolute paths and a dedicated database. It creates the root on first startup. `TERRA_AUTHOR` selects its author. Root scoping guards accidental misuse, not direct filesystem access.

## Verification

`node --test markdown/document-mcp-integration.mjs` exercises real stdio/RocksDB, compact reads, parsed writes, conflicts, deletion, history and links. See [agent experiments](../../markdown/experiments/README.md) for usability trials.

Search excludes Markdown delimiters and link destinations; labels and code contents remain searchable. Regexp patterns are limited to 2000 characters and execute in an isolated worker with a 2-second timeout. Invalid or timed-out expressions return an error rather than empty results.

## Project entry points

Any block can have an optional `entrypoint` label. Set it through `change` with
`{id, expected, entrypoint: "When to read this"}`; use null to remove it. Labels
are single-line, nonempty and at most 300 characters. Changes use the same CAS,
reason and history as other block edits; replacing text preserves the label.

Root `read` returns an `entrypoints` collection separately from the ordinary tree
view, in either format. It includes labels and live/pinned links, never target
bodies. Discovery scans the project at the read snapshot (10000 nodes, depth 64,
100 links returned) and reports completeness/truncation; there is no dedicated
index yet. The viewer shows these links under “Start here”. Deleted subtrees are
excluded; historical root reads recover their earlier entry points.
