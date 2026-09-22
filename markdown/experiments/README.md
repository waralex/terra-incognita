# Agent memory trials — 2026-09-18

Baseline: `d68924f`. List insertion fix: `4a05fe3`.
Trials used real stdio MCP and isolated temporary RocksDB databases; no production
project memory was edited. These are small qualitative trials, not a comparison
of models or proof of general reliability.

## Investigation handoff

The worker made 11 successful tool calls: created an investigation, corrected a
mistaken conclusion, expanded evidence into separate paragraphs, then renamed the
section and completed a checkbox atomically. A second agent received only DB
location, root ID, section ID and the recovery task. It did not read the worker's
script or logs.

The reader recovered the original TTL-regression diagnosis, the corrected
clock-skew diagnosis, the reason for the change and outstanding benchmark task.
It used 8 tool calls (5 read, including a historical read; 3 history), plus one
tools/list request. The original diagnosis was absent from current text, so
history was necessary. All measurements in this scenario were synthetic.

Reproduce the writer from repository root:

```sh
RUSTC_WRAPPER= cargo build -p terra-core --example document-bridge
node markdown/experiments/investigation-handoff.mjs
```

The script leaves a temporary DB and writes `/tmp/terra-memory-trial-handoff.json`.
Give only that handoff to a recovery agent; sharing the script/log would reveal
answers and invalidate the blind recovery exercise. This script records the
writer workflow, not an automated test of a fresh model's recovery capability.

## Conflicts and list insertion

A separate agent verified stale compound edits reject all changes, invalid
container replacement leaves the tree intact, and invalid mixed fragments roll
back atomically. It found that adding an item to an existing list was impossible:
the parser produced an extra List wrapper. The fix unwraps a single matching list
fragment when its destination is a List. Numbering is inherited from the existing
list; insertion under a ListItem still creates a nested list.

```sh
node markdown/experiments/list-insertion.mjs
node --test markdown/document-mcp-integration.mjs
```

Post-fix checks passed for ordered insertion, multiple appended items, nested
lists and checkbox states, and rejection of mismatched list types. Scripts keep
temporary evidence for inspection and must be run sequentially (fixed log paths).

## Interface lessons

- Checkbox previews avoid opening every item to identify the right task.
- Consistent read/history fields reduce translation work for agents.
- `changed_blocks` lets readers find siblings introduced by the same transaction.
- One parser command needs parent-aware behavior for insertion into containers.
