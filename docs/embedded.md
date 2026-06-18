# Embedded usage

When you want terra inside your Rust process — not behind an HTTP
boundary — depend on `terra-core` directly. `terra-server` is a thin
HTTP wrapper around the same library; embedding gives you typed Rust
structs, no (de)serialization, and sub-millisecond calls.

For concepts (transactions, branches, entities, managed items) see
[concepts.md](concepts.md). For the YAML schema format see
[configuration.md](configuration.md).

## Dependency

Not published to crates.io yet. Use a path or a git dependency:

```toml
[dependencies]
terra-core = { path = "../terra-incognita/crates/terra-core" }
# or
terra-core = { git = "https://github.com/<user>/<repo>.git", rev = "<sha>" }

# Optional: semantic similarity via ONNX embeddings.
# Without this feature the `entities.similar` query always returns empty.
terra-core = { path = "...", features = ["onnx"] }
```

## Opening a Terra instance

```rust
use std::sync::Arc;

use terra_core::config::{DataSchema, ProjectConfig};
use terra_core::embed::{Embedder, NoopEmbedder};
use terra_core::Terra;

let config = Arc::new(
    ProjectConfig::builder()
        .data_dir("./terra-data".into())
        .schema_path("./schema.yaml".into())
        .build(),
);

let schema = Arc::new(DataSchema::from_file(&config.schema_path)?);

let embedder: Arc<dyn Embedder> = Arc::new(NoopEmbedder);

let terra = Terra::open(&config.data_dir, config.clone(), schema, embedder)?;
```

`Terra` is `Send + Sync`; wrap in `Arc<Terra>` to share across threads
or async tasks.

## Writing a transaction

Every input type implements `Executable`, and `Terra::execute(branch,
input)` runs it atomically — writes are committed or rolled back as a
unit.

```rust
use serde_json::{Map, Value};
use terra_core::command::input::transaction::TransactionInput;
use terra_core::domain::entity::{Entity, PropertyValue};

fn reasoning(text: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("reasoning".into(), Value::String(text.into()));
    m
}

let input = TransactionInput::new(reasoning("first entity"))
    .create_entity(Entity::new(
        "alice".parse()?,
        Some(Value::String("a person I know".into())),
        vec![PropertyValue {
            property: "age".parse()?,
            value: Value::from(30),
            context: (),
        }],
        reasoning("told to me directly"),
    ));

let tx = terra.execute(&"main".parse()?, input)?;
println!("committed: {}", tx.context.tx_id);
```

Other builder methods on `TransactionInput`: `update_entity`,
`create_managed`, `update_managed`, `delete_entity(DeleteItem)`,
`touch(TouchItem)`.

## Reading

All read commands take the same `Terra::execute` path. Pick an input
type based on what you want:

| Input type | Output |
|---|---|
| `TouchedEntitiesQuery` | `Vec<Entity<TxMeta>>` — recently touched entities |
| `SimilarEntitiesQuery` | `Vec<SimilarEntity<TxMeta>>` — semantic search |
| `GrepEntitiesQuery` | `Vec<Entity<TxMeta>>` — regex search over slug/property/value/reasoning |
| `EntityHistoryQuery` | `Vec<EntityHistoryEntry>` — snapshots at each tx |
| `GetTransactionQuery` | `TransactionDetail` — full tx detail, cross-branch |
| `ListTransactionsQuery` | `Vec<Transaction<TxMeta>>` |
| `ListManagedQuery` | `Vec<Managed<TxMeta>>` |
| `GetBranchQuery` | `Branch<TxMeta>` |

```rust
use terra_core::command::input::touched_entities::TouchedEntitiesQuery;

let entities = terra.execute(
    &"main".parse()?,
    TouchedEntitiesQuery::new(None /* at_tx */, 10 /* limit */),
)?;

for entity in entities {
    println!("{} with {} properties", entity.slug, entity.properties.len());
    for pv in &entity.properties {
        println!("  {} = {}", pv.property, pv.value);
    }
}
```

## Slugs

Slugs come from `FromStr`:

```rust
use terra_core::io::slug::Slug;

let branch: Slug = "main".parse()?;
let entity: Slug = "alice".parse()?;
```

An invalid slug (empty, wrong characters, too long) returns a
`SlugError`.

## Embedder

The embedder is behind an `Arc<dyn Embedder>` trait object. Two
implementations ship with terra-core:

- **`NoopEmbedder`** — always on, produces zero-dimensional vectors.
  `entities.similar` returns an empty result. Use for testing or when
  semantic search is not needed.
- **`OnnxEmbedder`** — requires the `onnx` feature. Point it at a
  directory containing `model.onnx` and `tokenizer.json`
  (e.g. `all-MiniLM-L6-v2`):

  ```rust
  #[cfg(feature = "onnx")]
  use terra_core::embed::OnnxEmbedder;
  use std::path::Path;

  let embedder: Arc<dyn Embedder> =
      Arc::new(OnnxEmbedder::from_dir(Path::new("./models/all-MiniLM-L6-v2"))?);
  ```

## Errors

Every fallible terra-core call returns `Result<T, DbError>`:

```rust
use terra_core::io::DbError;

match terra.execute(&branch, input) {
    Ok(tx) => {/* ... */}
    Err(DbError::Validation(e)) => eprintln!("schema violation: {e}"),
    Err(DbError::Storage(msg)) => eprintln!("storage: {msg}"),
}
```

`Validation` wraps `ValidationError` (missing required meta field,
unknown managed type, duplicate slug in one transaction, invalid
state transition, etc.). `Storage` covers RocksDB-level failures,
not-found lookups, and conflicts (existence / deletion state).

## Rustdoc

`cargo doc --open -p terra-core` renders the public API with
module-level docs and type docstrings.
