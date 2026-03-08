/// Property slugs reserved by the system (`entity-uuid`, `entity-name`, `entity-type`).
pub const RESERVED_PROPERTIES: &[&str] = &["entity-uuid", "entity-name", "entity-type"];

/// Returns `true` if the slug is a reserved property name.
pub fn is_reserved(slug: &str) -> bool {
    RESERVED_PROPERTIES.contains(&slug)
}
