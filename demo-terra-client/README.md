# demo-terra-client

An exploratory POC that wires an LLM to [terra-server](../crates/terra-server)
as persistent memory. It serves a small web UI where you chat with
an agent whose long-term memory lives in terra — every turn is
ingested as a transaction, recent touches and similar entities are
loaded as context for the next call.

Not a reference integration and not maintained as a product — this
is the working example that drove v0.2 of terra-core, kept around
as the shortest path from zero to a talking agent with terra
behind it.

## Prerequisites

- Node.js 20+
- Rust toolchain (for building terra-server from this repo)
- An API key for one of the supported LLM providers
  - Anthropic (default) — `ANTHROPIC_API_KEY`
  - OpenAI — `OPENAI_API_KEY`

## Quickstart

```bash
# 1. Install Node dependencies
npm install

# 2. Start terra-server in one shell
#    This uses the bundled config in ./terra/ and builds terra-server with the onnx feature
#    (semantic similarity). Use `npm run terra:no-embed` to skip embeddings.
npm run terra

# 3. In another shell, start the demo client
export ANTHROPIC_API_KEY=sk-ant-...
npm run dev
```

Then open <http://localhost:3001>. The demo app talks to terra-server
on `http://localhost:3000` and to the LLM provider over the internet.

## Configuration

Environment variables (all optional unless noted):

| Variable | Default | Notes |
|---|---|---|
| `TERRA_SERVER_URL` | `http://localhost:3000` | Where terra-server is listening |
| `TERRA_BRANCH` | `main` | Branch used for all reads and writes |
| `PORT` | `3001` | Port for the demo's own web UI |
| `LLM_PROVIDER` | `anthropic` | `anthropic` or `openai` |
| `LLM_MODEL` | provider default | Override the model id |
| `ANTHROPIC_API_KEY` | — | Required if `LLM_PROVIDER=anthropic` |
| `OPENAI_API_KEY` | — | Required if `LLM_PROVIDER=openai` |
| `CONTEXT_TRANSACTIONS` | `10` | Recent transactions loaded into LLM context |
| `CONTEXT_ENTITIES` | `20` | Recently touched entities loaded into LLM context |
| `SIMILAR_ENTITIES` | `20` | Max semantic-search results per user message |
| `SIMILAR_MIN_SCORE` | `0.7` | Similarity threshold for `entities.similar` |
| `LOG_LLM` | `false` | Log raw LLM requests and responses to stderr |

## Models

- **Default provider:** Anthropic.
- **Default model:** `claude-sonnet-4-20250514` (Sonnet 4). This is
  what was actually tested while iterating on the demo. Other
  Anthropic models and OpenAI (`gpt-4o` default) should work but
  have not been exercised.
- **Translation:** the demo translates non-English user messages to
  English before the main LLM call, using Anthropic Haiku
  (`claude-haiku-4-5-20251001`). This path requires
  `ANTHROPIC_API_KEY` regardless of the primary provider.

## Bundled terra schema

The demo's terra instance is configured by files in `terra/`:

- `terra/terra-server.yaml` — points at `terra/project.yaml` and
  the ONNX model at `../models/all-MiniLM-L6-v2` (relative to the
  config file).
- `terra/project.yaml` — `data_dir: data`, `schema_path: schema.yaml`.
- `terra/schema.yaml` — declares `transaction_meta` (reasoning /
  question / answer), `entity_change_meta.reasoning`,
  `branch_meta.reasoning`, and a `rule` managed type with a
  draft / active / rejected / promoted lifecycle.

The `rule` managed type is used by the demo to persist
self-improving agent instructions across conversations — the agent
can create, promote, and reject its own rules.

See [../docs/configuration.md](../docs/configuration.md) for the
full schema format.

## Scripts

- `npm run terra` — start terra-server (with `onnx` feature for
  embeddings).
- `npm run terra:no-embed` — start terra-server without embeddings.
  Semantic similarity returns empty in this mode.
- `npm run dev` — start the demo server in dev mode (tsx).
- `npm run build` / `npm start` — compile TypeScript and run the
  built server.
