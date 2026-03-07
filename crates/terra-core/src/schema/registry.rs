use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;
use uuid::Uuid;

use crate::schema::entity_property::{EntityProperty, ValueType};
use crate::schema::entity_type::EntityType;
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

    pub fn create_entity_type(&self, slug: &str) -> Result<EntityType, SchemaError> {
        validate_slug(slug)?;

        let id = Uuid::now_v7();
        let now = Utc::now();
        let created_at_str = now.to_rfc3339();

        self.conn
            .execute(
                "INSERT INTO entity_types (id, slug, created_at) VALUES (?1, ?2, ?3)",
                params![id.as_bytes().as_slice(), slug, created_at_str],
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
            created_at: now,
        })
    }

    pub fn create_property(
        &self,
        slug: &str,
        value_type: ValueType,
    ) -> Result<EntityProperty, SchemaError> {
        validate_slug(slug)?;

        let id = Uuid::now_v7();
        let now = Utc::now();
        let created_at_str = now.to_rfc3339();

        self.conn
            .execute(
                "INSERT INTO entity_properties (id, slug, value_type, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id.as_bytes().as_slice(), slug, value_type.as_str(), created_at_str],
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
            "SELECT p.id, p.slug, p.value_type, p.created_at
             FROM entity_properties p
             JOIN entity_type_properties tp ON tp.entity_property_id = p.id
             WHERE tp.entity_type_id = ?1
             ORDER BY p.slug",
        )?;

        let props = stmt
            .query_map(params![type_id], |row| {
                let id_bytes: Vec<u8> = row.get(0)?;
                let slug: String = row.get(1)?;
                let vt_str: String = row.get(2)?;
                let created_at_str: String = row.get(3)?;
                Ok((id_bytes, slug, vt_str, created_at_str))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        props
            .into_iter()
            .map(|(id_bytes, slug, vt_str, created_at_str)| {
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
        let et = reg.create_entity_type("military-unit").unwrap();
        assert_eq!(et.slug, "military-unit");
        assert_eq!(et.id.get_version(), Some(uuid::Version::SortRand));
    }

    #[test]
    fn reject_invalid_slug() {
        let reg = registry();
        assert!(matches!(
            reg.create_entity_type("Invalid"),
            Err(SchemaError::InvalidSlug(_))
        ));
        assert!(matches!(
            reg.create_entity_type(""),
            Err(SchemaError::InvalidSlug(_))
        ));
        assert!(matches!(
            reg.create_entity_type("-leading"),
            Err(SchemaError::InvalidSlug(_))
        ));
    }

    #[test]
    fn reject_duplicate_entity_type() {
        let reg = registry();
        reg.create_entity_type("unit").unwrap();
        assert!(matches!(
            reg.create_entity_type("unit"),
            Err(SchemaError::DuplicateEntityType(_))
        ));
    }

    #[test]
    fn create_property_and_attach() {
        let reg = registry();
        reg.create_entity_type("military-unit").unwrap();
        let prop = reg.create_property("unit-name", ValueType::String).unwrap();
        assert_eq!(prop.slug, "unit-name");
        assert_eq!(prop.value_type, ValueType::String);

        reg.attach_property("military-unit", "unit-name").unwrap();

        let props = reg.list_properties("military-unit").unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].slug, "unit-name");
        assert_eq!(props[0].value_type, ValueType::String);
    }

    #[test]
    fn reject_duplicate_property() {
        let reg = registry();
        reg.create_property("name", ValueType::String).unwrap();
        assert!(matches!(
            reg.create_property("name", ValueType::Number),
            Err(SchemaError::DuplicateProperty(_))
        ));
    }

    #[test]
    fn attach_to_nonexistent_entity_type() {
        let reg = registry();
        reg.create_property("name", ValueType::String).unwrap();
        assert!(matches!(
            reg.attach_property("no-such-type", "name"),
            Err(SchemaError::EntityTypeNotFound(_))
        ));
    }

    #[test]
    fn attach_nonexistent_property() {
        let reg = registry();
        reg.create_entity_type("unit").unwrap();
        assert!(matches!(
            reg.attach_property("unit", "no-such-prop"),
            Err(SchemaError::PropertyNotFound(_))
        ));
    }

    #[test]
    fn many_to_many_property_attachment() {
        let reg = registry();
        reg.create_entity_type("unit").unwrap();
        reg.create_entity_type("location").unwrap();
        reg.create_property("name", ValueType::String).unwrap();
        reg.create_property("code", ValueType::Number).unwrap();

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
        reg.create_entity_type("empty").unwrap();
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
}
