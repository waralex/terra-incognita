CREATE TABLE IF NOT EXISTS entity_types (
    id BLOB PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TEXT NOT NULL
);
