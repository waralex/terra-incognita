pub const RESERVED_PROPERTIES: &[&str] = &["entity-uuid", "entity-name", "entity-type"];

pub fn is_reserved(slug: &str) -> bool {
    RESERVED_PROPERTIES.contains(&slug)
}
