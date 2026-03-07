CREATE TABLE IF NOT EXISTS entity_type_properties (
    entity_type_id BLOB NOT NULL REFERENCES entity_types(id),
    entity_property_id BLOB NOT NULL REFERENCES entity_properties(id),
    PRIMARY KEY (entity_type_id, entity_property_id)
);
