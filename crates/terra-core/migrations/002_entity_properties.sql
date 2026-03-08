CREATE TABLE IF NOT EXISTS entity_properties (
    id BLOB PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    description TEXT,
    value_type TEXT NOT NULL CHECK(value_type IN ('set', 'struct', 'range')),
    created_at TEXT NOT NULL
);
