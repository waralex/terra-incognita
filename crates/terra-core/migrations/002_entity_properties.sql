CREATE TABLE IF NOT EXISTS entity_properties (
    id BLOB PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    value_type TEXT NOT NULL CHECK(value_type IN ('string', 'number')),
    created_at TEXT NOT NULL
);
