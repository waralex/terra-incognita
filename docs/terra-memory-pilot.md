# Optional project memory

Current memory uses the shared document service: MCP RPC on localhost:8097. See [service setup](engine/document-mcp.md).

Use a registered project's existing `.local/memory/<project>/config.json`. It
contains its project slug, the service URL and a credential. Do not commit it.
Several chats/worktrees can share that config and the same project memory.

Project MCP configuration (preserve other servers):

```json
{"mcpServers":{"terra-memory":{"command":"node","args":["/absolute/path/to/terra-incognita/scripts/terra-document-mcp-proxy.mjs","/absolute/path/to/project-memory.json"]}}}
```

This directly starts the proxy visible in the process list.

Link `skills/terra-memory` into the agent's skill directory. Invoke `/terra-memory`
in Claude to enable it for the current chat. Reconnect MCP after changing server
configuration or tool schemas. This is an instruction-level choice; it does not
change the host's global memory setting. Existing file notes remain a source to
verify, not a second write destination.

Alternatively, start/resume a Claude chat from the task's worktree:

```sh
node /absolute/path/to/terra-incognita/scripts/terra-memory-chat.mjs cube
```

The launcher adds the project MCP and instructions to that process. It requires
an already registered project; it does not migrate notes or start research agents.
Pass `-- --resume` to choose a previous conversation.

## Working with memory

`read()` opens the root. Open child IDs or increase depth for a whole section.
`write` parses Markdown into blocks, `change` groups title/checkbox edits, and
`remove` deletes obsolete current content while preserving history. Copy revisions
from reads or write receipts; a conflict requires rereading and reconsidering.
Search reports whether the tree is empty and whether results are complete.

Current history is per block, with transaction reasons and related block IDs.

Disconnecting MCP preserves data. Use ordinary work to evaluate whether memory
reduces repeated investigation; avoid creating a separate reporting ritual.
