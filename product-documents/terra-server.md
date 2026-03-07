# terra-server

HTTP server for the schema registry. Single endpoint, YAML in/out.

## Quick Start

```bash
cargo run -p terra-server
```

Server starts on `0.0.0.0:3000` by default.

## Endpoint

```
POST /query
Content-Type: application/yaml
```

Accepts YAML commands, returns YAML responses. See [query-specification.md](query-specification.md).

## Configuration

### Config File: `terra-incognita.yml`

```yaml
port: 3000                    # TCP port (default: 3000)
data_dir: .terra-incognita    # data directory (default: .terra-incognita)
```

Both fields are optional. Missing fields use defaults.

### Config File Lookup (in priority order)

1. `./terra-incognita.yml` — current working directory
2. `$TERRA_INCOGNITA_CONFIG` — path from environment variable
3. `~/.terra-incognita/terra-incognita.yml` — home directory

If no config file is found, defaults are used.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `TERRA_INCOGNITA_CONFIG` | Override config file path |
| `RUST_LOG` | Logging level (via `tracing_subscriber`) |

## Defaults

| Parameter | Default | Description |
|-----------|---------|-------------|
| `port` | `3000` | TCP port |
| `data_dir` | `.terra-incognita` | Directory for database files |
| config filename | `terra-incognita.yml` | Config file name |

## Data Files

| Path | Description |
|------|-------------|
| `{data_dir}/schema.db` | SQLite database with schema registry |

The `data_dir` is created automatically if it doesn't exist.

## Logging

Server logs config path (if found), `data_dir`, `port`, and request handling via `tracing`.
Configure verbosity with `RUST_LOG`:

```bash
RUST_LOG=debug cargo run -p terra-server
```
