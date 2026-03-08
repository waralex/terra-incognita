use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;
use uuid::Uuid;

use crate::schema::entity_property::{EntityProperty, ValueType};
use crate::schema::entity_type::EntityType;
use crate::schema::reserved;
use crate::schema::slug::validate_slug;
use crate::schema::SchemaError;

pub struct SchemaRegistry {
    conn: Connection,
}

impl SchemaRegistry {
    pub fn open(path: &Path) -> Result<Self, SchemaError> {
        let conn = Connection::open(path)?;
        let registry = Self { conn };
        registry.run_migrations()?;
        Ok(registry)
    }

    pub fn open_in_memory() -> Result<Self, SchemaError> {
        let conn = Connection::open_in_memory()?;
        let registry = Self { conn };
        registry.run_migrations()?;
        Ok(registry)
    }

    fn run_migrations(&self) -> Result<(), SchemaError> {
        for ddl in crate::schema::migrations::ALL {
            self.conn.execute(ddl, [])?;
        }
        Ok(())
    }

    pub fn create_entity_type(
        &self,
        slug: &str,
        description: Option<&str>,
    ) -> Result<EntityType, SchemaError> {
        validate_slug(slug)?;

        let id = Uuid::now_v7();
        let now = Utc::now();
        let created_at_str = now.to_rfc3339();

        self.conn
            .execute(
                "INSERT INTO entity_types (id, slug, description, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id.as_bytes().as_slice(), slug, description, created_at_str],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    SchemaError::DuplicateEntityType(slug.to_string())
                }
                other => SchemaError::Db(other),
            })?;

        Ok(EntityType {
            id,
            slug: slug.to_string(),
            description: description.map(String::from),
            created_at: now,
        })
    }

    pub fn create_property(
        &self,
        slug: &str,
        value_type: ValueType,
        description: Option<&str>,
    ) -> Result<EntityProperty, SchemaError> {
        validate_slug(slug)?;
        if reserved::is_reserved(slug) {
            return Err(SchemaError::ReservedProperty(slug.to_string()));
        }

        let id = Uuid::now_v7();
        let now = Utc::now();
        let created_at_str = now.to_rfc3339();

        self.conn
            .execute(
                "INSERT INTO entity_properties (id, slug, description, value_type, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id.as_bytes().as_slice(), slug, description, value_type.as_str(), created_at_str],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    SchemaError::DuplicateProperty(slug.to_string())
                }
                other => SchemaError::Db(other),
            })?;

        Ok(EntityProperty {
            id,
            slug: slug.to_string(),
            description: description.map(String::from),
            value_type,
            created_at: now,
        })
    }

    pub fn attach_property(
        &self,
        entity_type_slug: &str,
        property_slug: &str,
    ) -> Result<(), SchemaError> {
        let type_id: Vec<u8> = self
            .conn
            .query_row(
                "SELECT id FROM entity_types WHERE slug = ?1",
                params![entity_type_slug],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    SchemaError::EntityTypeNotFound(entity_type_slug.to_string())
                }
                other => SchemaError::Db(other),
            })?;

        let prop_id: Vec<u8> = self
            .conn
            .query_row(
                "SELECT id FROM entity_properties WHERE slug = ?1",
                params![property_slug],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    SchemaError::PropertyNotFound(property_slug.to_string())
                }
                other => SchemaError::Db(other),
            })?;

        self.conn.execute(
            "INSERT OR IGNORE INTO entity_type_properties (entity_type_id, entity_property_id) VALUES (?1, ?2)",
            params![type_id, prop_id],
        )?;

        Ok(())
    }

    pub fn list_entity_types(&self) -> Result<Vec<EntityType>, SchemaError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, slug, description, created_at FROM entity_types ORDER BY slug")?;

        let rows = stmt
            .query_map([], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let slug: String = row.get(1)?;
                let description: Option<String> = row.get(2)?;
                let created_at_str: String = row.get(3)?;
                Ok((id_bytes, slug, description, created_at_str))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter()
            .map(|(id_bytes, slug, description, created_at_str)| {
                let id = Uuid::from_slice(&id_bytes).map_err(|e| {
                    SchemaError::Db(rusqlite::Error::InvalidParameterName(e.to_string()))
                })?;
                let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .map_err(|e| {
                        SchemaError::Db(rusqlite::Error::InvalidParameterName(e.to_string()))
                    })?;
                Ok(EntityType {
                    id,
                    slug,
                    description,
                    created_at,
                })
            })
            .collect()
    }

    pub fn get_entity_type(&self, slug: &str) -> Result<EntityType, SchemaError> {
        let (id_bytes, description, created_at_str): (Vec<u8>, Option<String>, String) = self
            .conn
            .query_row(
                "SELECT id, description, created_at FROM entity_types WHERE slug = ?1",
                params![slug],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    SchemaError::EntityTypeNotFound(slug.to_string())
                }
                other => SchemaError::Db(other),
            })?;

        let id = Uuid::from_slice(&id_bytes)
            .map_err(|e| SchemaError::Db(rusqlite::Error::InvalidParameterName(e.to_string())))?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| SchemaError::Db(rusqlite::Error::InvalidParameterName(e.to_string())))?;

        Ok(EntityType {
            id,
            slug: slug.to_string(),
            description,
            created_at,
        })
    }

    pub fn list_all_properties(&self) -> Result<Vec<EntityProperty>, SchemaError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, slug, description, value_type, created_at FROM entity_properties ORDER BY slug",
        )?;

        let rows = stmt
            .query_map([], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let slug: String = row.get(1)?;
                let description: Option<String> = row.get(2)?;
                let vt_str: String = row.get(3)?;
                let created_at_str: String = row.get(4)?;
                Ok((id_bytes, slug, description, vt_str, created_at_str))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        rows.into_iter()
            .map(|(id_bytes, slug, description, vt_str, created_at_str)| {
                let id = Uuid::from_slice(&id_bytes).map_err(|e| {
                    SchemaError::Db(rusqlite::Error::InvalidParameterName(e.to_string()))
                })?;
                let value_type = ValueType::from_str(&vt_str)
                    .ok_or_else(|| SchemaError::InvalidSlug(vt_str.clone()))?;
                let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .map_err(|_| SchemaError::InvalidSlug(created_at_str))?;
                Ok(EntityProperty {
                    id,
                    slug,
                    description,
                    value_type,
                    created_at,
                })
            })
            .collect()
    }

    pub fn list_properties(
        &self,
        entity_type_slug: &str,
    ) -> Result<Vec<EntityProperty>, SchemaError> {
        let type_id: Vec<u8> = self
            .conn
            .query_row(
                "SELECT id FROM entity_types WHERE slug = ?1",
                params![entity_type_slug],
                |row| row.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    SchemaError::EntityTypeNotFound(entity_type_slug.to_string())
                }
                other => SchemaError::Db(other),
            })?;

        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.slug, p.description, p.value_type, p.created_at
             FROM entity_properties p
             JOIN entity_type_properties tp ON tp.entity_property_id = p.id
             WHERE tp.entity_type_id = ?1
             ORDER BY p.slug",
        )?;

        let props = stmt
            .query_map(params![type_id], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let slug: String = row.get(1)?;
                let description: Option<String> = row.get(2)?;
                let vt_str: String = row.get(3)?;
                let created_at_str: String = row.get(4)?;
                Ok((id_bytes, slug, description, vt_str, created_at_str))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        props
            .into_iter()
            .map(|(id_bytes, slug, description, vt_str, created_at_str)| {
                let id = Uuid::from_slice(&id_bytes)
                    .map_err(|e| SchemaError::Db(rusqlite::Error::InvalidParameterName(e.to_string())))?;
                let value_type = ValueType::from_str(&vt_str)
                    .ok_or_else(|| SchemaError::InvalidSlug(vt_str.clone()))?;
                let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .map_err(|_| SchemaError::InvalidSlug(created_at_str))?;
                Ok(EntityProperty {
                    id,
                    slug,
                    description,
                    value_type,
                    created_at,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> SchemaRegistry {
        SchemaRegistry::open_in_memory().unwrap()
    }

    #[test]
    fn create_entity_type() {
        let reg = registry();
        let et = reg.create_entity_type("military-unit", None).unwrap();
        assert_eq!(et.slug, "military-unit");
        assert!(et.description.is_none());
        assert_eq!(et.id.get_version(), Some(uuid::Version::SortRand));
    }

    #[test]
    fn create_entity_type_with_description() {
        let reg = registry();
        let et = reg
            .create_entity_type("tank", Some("Armored fighting vehicle"))
            .unwrap();
        assert_eq!(et.slug, "tank");
        assert_eq!(et.description.as_deref(), Some("Armored fighting vehicle"));
    }

    #[test]
    fn reject_invalid_slug() {
        let reg = registry();
        assert!(matches!(
            reg.create_entity_type("Invalid", None),
            Err(SchemaError::InvalidSlug(_))
        ));
        assert!(matches!(
            reg.create_entity_type("", None),
            Err(SchemaError::InvalidSlug(_))
        ));
        assert!(matches!(
            reg.create_entity_type("-leading", None),
            Err(SchemaError::InvalidSlug(_))
        ));
    }

    #[test]
    fn reject_duplicate_entity_type() {
        let reg = registry();
        reg.create_entity_type("unit", None).unwrap();
        assert!(matches!(
            reg.create_entity_type("unit", None),
            Err(SchemaError::DuplicateEntityType(_))
        ));
    }

    #[test]
    fn create_property_and_attach() {
        let reg = registry();
        reg.create_entity_type("military-unit", None).unwrap();
        let prop = reg
            .create_property("unit-name", ValueType::Struct, None)
            .unwrap();
        assert_eq!(prop.slug, "unit-name");
        assert_eq!(prop.value_type, ValueType::Struct);

        reg.attach_property("military-unit", "unit-name").unwrap();

        let props = reg.list_properties("military-unit").unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].slug, "unit-name");
        assert_eq!(props[0].value_type, ValueType::Struct);
    }

    #[test]
    fn create_property_with_description() {
        let reg = registry();
        let prop = reg
            .create_property("armor-class", ValueType::Struct, Some("Protection level rating"))
            .unwrap();
        assert_eq!(prop.description.as_deref(), Some("Protection level rating"));
    }

    #[test]
    fn reject_duplicate_property() {
        let reg = registry();
        reg.create_property("name", ValueType::Struct, None)
            .unwrap();
        assert!(matches!(
            reg.create_property("name", ValueType::Range, None),
            Err(SchemaError::DuplicateProperty(_))
        ));
    }

    #[test]
    fn attach_to_nonexistent_entity_type() {
        let reg = registry();
        reg.create_property("name", ValueType::Struct, None)
            .unwrap();
        assert!(matches!(
            reg.attach_property("no-such-type", "name"),
            Err(SchemaError::EntityTypeNotFound(_))
        ));
    }

    #[test]
    fn attach_nonexistent_property() {
        let reg = registry();
        reg.create_entity_type("unit", None).unwrap();
        assert!(matches!(
            reg.attach_property("unit", "no-such-prop"),
            Err(SchemaError::PropertyNotFound(_))
        ));
    }

    #[test]
    fn many_to_many_property_attachment() {
        let reg = registry();
        reg.create_entity_type("unit", None).unwrap();
        reg.create_entity_type("location", None).unwrap();
        reg.create_property("name", ValueType::Struct, None)
            .unwrap();
        reg.create_property("code", ValueType::Range, None)
            .unwrap();

        reg.attach_property("unit", "name").unwrap();
        reg.attach_property("unit", "code").unwrap();
        reg.attach_property("location", "name").unwrap();

        let unit_props = reg.list_properties("unit").unwrap();
        assert_eq!(unit_props.len(), 2);

        let loc_props = reg.list_properties("location").unwrap();
        assert_eq!(loc_props.len(), 1);
        assert_eq!(loc_props[0].slug, "name");
    }

    #[test]
    fn list_properties_empty() {
        let reg = registry();
        reg.create_entity_type("empty", None).unwrap();
        let props = reg.list_properties("empty").unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn list_properties_nonexistent_type() {
        let reg = registry();
        assert!(matches!(
            reg.list_properties("no-such-type"),
            Err(SchemaError::EntityTypeNotFound(_))
        ));
    }

    #[test]
    fn list_entity_types_empty() {
        let reg = registry();
        let types = reg.list_entity_types().unwrap();
        assert!(types.is_empty());
    }

    #[test]
    fn list_entity_types_returns_all() {
        let reg = registry();
        reg.create_entity_type("bravo", None).unwrap();
        reg.create_entity_type("alpha", None).unwrap();
        let types = reg.list_entity_types().unwrap();
        assert_eq!(types.len(), 2);
        assert_eq!(types[0].slug, "alpha");
        assert_eq!(types[1].slug, "bravo");
    }

    #[test]
    fn get_entity_type_found() {
        let reg = registry();
        let created = reg.create_entity_type("tank", None).unwrap();
        let fetched = reg.get_entity_type("tank").unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.slug, "tank");
    }

    #[test]
    fn get_entity_type_not_found() {
        let reg = registry();
        assert!(matches!(
            reg.get_entity_type("ghost"),
            Err(SchemaError::EntityTypeNotFound(_))
        ));
    }

    #[test]
    fn list_all_properties_empty() {
        let reg = registry();
        let props = reg.list_all_properties().unwrap();
        assert!(props.is_empty());
    }

    #[test]
    fn list_all_properties_returns_all() {
        let reg = registry();
        reg.create_property("name", ValueType::Struct, None)
            .unwrap();
        reg.create_property("count", ValueType::Range, None)
            .unwrap();
        let props = reg.list_all_properties().unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].slug, "count");
        assert_eq!(props[1].slug, "name");
    }

    #[test]
    fn get_entity_type_preserves_description() {
        let reg = registry();
        reg.create_entity_type("tank", Some("Armored vehicle")).unwrap();
        let fetched = reg.get_entity_type("tank").unwrap();
        assert_eq!(fetched.description.as_deref(), Some("Armored vehicle"));
    }

    #[test]
    fn reject_reserved_property() {
        let reg = registry();
        for slug in &["entity-uuid", "entity-name", "entity-type"] {
            assert!(matches!(
                reg.create_property(slug, ValueType::Struct, None),
                Err(SchemaError::ReservedProperty(_))
            ));
        }
    }

    #[test]
    fn list_properties_preserves_description() {
        let reg = registry();
        reg.create_entity_type("unit", None).unwrap();
        reg.create_property("name", ValueType::Struct, Some("Display name"))
            .unwrap();
        reg.attach_property("unit", "name").unwrap();
        let props = reg.list_properties("unit").unwrap();
        assert_eq!(props[0].description.as_deref(), Some("Display name"));
    }
}
