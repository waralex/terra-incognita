# terra-cli

Pipe-friendly YAML query client for terra-server. Reads YAML from stdin, sends
POST to server, prints response to stdout.

## Usage

```bash
terra-cli [URL]
```

- `URL` — server endpoint (default: `http://localhost:3000/query`)

## Examples

```bash
# Default URL (localhost:3000)
echo "verb: list
target: entity-type" | terra-cli

# Custom URL
echo "verb: list
target: entity-type" | terra-cli http://staging:4000/query

# From file
terra-cli < list-types.yml

# Pipe with file and custom server
terra-cli http://remote:3000/query < create-tank.yml
```

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Error (connection, HTTP error, empty input) |

Errors are printed to stderr. Server error responses (YAML) go to stdout with
exit code 1.

## Error Messages

- `error reading stdin: ...` — stdin read failure
- `error: empty input` — no data on stdin
- `error: ...` — HTTP/connection error

---

# terra-incognita.nvim

Neovim plugin for interactive work with terra-server. Three-panel layout:
tree (connections/queries), YAML query editor, YAML result viewer.

## Installation

Add `nvim-plugin/` to runtimepath:

```lua
-- init.lua
vim.opt.runtimepath:append("/path/to/terra-incognita/nvim-plugin")
require("terra-incognita").setup()
```

With lazy.nvim (local dev):

```lua
{ dir = "/path/to/terra-incognita/nvim-plugin" }
```

## Setup

```lua
require("terra-incognita").setup({
  terra_cli = "terra-cli",                    -- binary path (default: in PATH)
  data_dir = "~/.terra-incognita/nvim",       -- plugin data directory
  keymap_execute = "<leader>te",              -- execute query
  keymap_toggle = "<leader>tt",               -- toggle UI
})
```

All options are optional; defaults shown above.

## Commands

| Command | Description |
|---------|-------------|
| `:Terra` | Toggle UI (tree + splits) |
| `:TerraExecute` | Execute current query |

## Layout

```
┌──────────┬─────────────────┬─────────────────┐
│  tree    │  query (yaml)   │  result (yaml)  │
│  30 col  │    50%          │     50%         │
└──────────┴─────────────────┴─────────────────┘
```

- **Tree** — scratch buffer, connection/query browser
- **Query** — real file buffer (`ft=yaml`), auto-saved before execution
- **Result** — scratch buffer (`ft=yaml`), read-only

## Keymaps

### Global

| Key | Action |
|-----|--------|
| `<leader>tt` | Toggle UI |
| `<leader>te` | Execute query |

### Tree Buffer

| Key | Action |
|-----|--------|
| `Enter` | Toggle connection / open query / create new query |
| `a` | Add connection (prompts for name and port) |
| `d` | Delete query (with confirmation) |
| `D` | Delete connection (with confirmation) |
| `q` | Close UI |

## Data Storage

All data in `~/.terra-incognita/nvim/`:

```
~/.terra-incognita/nvim/
├── connections.yml              # connection list
└── queries/
    ├── local/                   # queries for "local" connection
    │   ├── list-types.yml
    │   └── create-tank.yml
    └── staging/                 # queries for "staging" connection
        └── get-unit.yml
```

### connections.yml

```yaml
- name: local
  port: 3000
- name: staging
  port: 4000
```

## Workflow

1. `:Terra` — open UI
2. `a` — add connection (e.g. `local`, port `3000`)
3. `Enter` on connection — expand
4. `Enter` on `+ new query` — name it, write YAML query
5. `<leader>te` — execute, result appears in right panel
