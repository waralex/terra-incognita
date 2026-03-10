use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::Path;
use uuid::Uuid;

use crate::schema::entity_property::{EntityProperty, ValueType};
use crate::schema::entity_type::EntityType;
use crate::schema::reserved;
use crate::schema::slug::validate_slug;
use crate::schema::SchemaError;

/// Input for creating an entity type (single item in a batch).
pub struct EntityTypeInput<'a> {
    pub slug: &'a str,
    pub description: Option<&'a str>,
    /// Property slugs to attach to the new entity type.
    pub properties: &'a [&'a str],
}

/// Input for creating a property (single item in a batch).
pub struct PropertyInput<'a> {
    pub slug: &'a str,
    pub value_type: ValueType,
    pub description: Option<&'a str>,
    /// Entity type slugs to attach this property to.
    pub entity_types: &'a [&'a str],
}

/// Input for attaching an existing property to an existing entity type.
pub struct AttachInput<'a> {
    pub entity_type: &'a str,
    pub property: &'a str,
}

/// SQLite-backed registry for entity types and their properties.
pub struct SchemaRegistry {
    conn: Connection,
}

impl SchemaRegistry {
    /// Opens a schema registry at the given path, creating it if needed.
    pub fn open(path: &Path) -> Result<Self, SchemaError> {
        let conn = Connection::open(path)?;
        let registry = Self { conn };
        registry.run_migrations()?;
        Ok(registry)
    }

    /// Opens an in-memory schema registry (for testing).
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

    /// Creates a single entity type with the given slug.
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

    /// Creates a single property with the given slug and value type.
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

    /// Attaches an existing property to an existing entity type.
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

    /// Attaches properties to entity types in a single transaction. All-or-nothing.
    pub fn attach_properties_batch(
        &mut self,
        items: &[AttachInput<'_>],
    ) -> Result<usize, SchemaError> {
        for (i, item) in items.iter().enumerate() {
            self.conn
                .query_row(
                    "SELECT 1 FROM entity_types WHERE slug = ?1",
                    params![item.entity_type],
                    |_| Ok(()),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => SchemaError::BatchItemError {
                        index: i,
                        source: Box::new(SchemaError::EntityTypeNotFound(
                            item.entity_type.to_string(),
                        )),
                    },
                    other => SchemaError::Db(other),
                })?;

            self.conn
                .query_row(
                    "SELECT 1 FROM entity_properties WHERE slug = ?1",
                    params![item.property],
                    |_| Ok(()),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => SchemaError::BatchItemError {
                        index: i,
                        source: Box::new(SchemaError::PropertyNotFound(
                            item.property.to_string(),
                        )),
                    },
                    other => SchemaError::Db(other),
                })?;
        }

        let tx = self.conn.transaction()?;

        for item in items {
            let type_id: Vec<u8> = tx
                .query_row(
                    "SELECT id FROM entity_types WHERE slug = ?1",
                    params![item.entity_type],
                    |row| row.get(0),
                )?;

            let prop_id: Vec<u8> = tx
                .query_row(
                    "SELECT id FROM entity_properties WHERE slug = ?1",
                    params![item.property],
                    |row| row.get(0),
                )?;

            tx.execute(
                "INSERT OR IGNORE INTO entity_type_properties (entity_type_id, entity_property_id) VALUES (?1, ?2)",
                params![type_id, prop_id],
            )?;
        }

        tx.commit()?;
        Ok(items.len())
    }

    /// Lists all entity types, ordered by slug.
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

    /// Retrieves a single entity type by UUID.
    pub fn get_entity_type_by_id(&self, id: &Uuid) -> Result<EntityType, SchemaError> {
        let (slug, description, created_at_str): (String, Option<String>, String) = self
            .conn
            .query_row(
                "SELECT slug, description, created_at FROM entity_types WHERE id = ?1",
                params![id.as_bytes().as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    SchemaError::EntityTypeNotFound(id.to_string())
                }
                other => SchemaError::Db(other),
            })?;

        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| SchemaError::Db(rusqlite::Error::InvalidParameterName(e.to_string())))?;

        Ok(EntityType {
            id: *id,
            slug,
            description,
            created_at,
        })
    }

    /// Retrieves a single entity type by slug.
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

    /// Lists all properties across all entity types, ordered by slug.
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

    /// Creates entity types in a single transaction with optional property attachment. All-or-nothing.
    pub fn create_entity_types_batch(
        &mut self,
        items: &[EntityTypeInput<'_>],
    ) -> Result<Vec<EntityType>, SchemaError> {
        for (i, item) in items.iter().enumerate() {
            validate_slug(item.slug).map_err(|e| SchemaError::BatchItemError {
                index: i,
                source: Box::new(e),
            })?;
            for prop_slug in item.properties {
                self.conn
                    .query_row(
                        "SELECT 1 FROM entity_properties WHERE slug = ?1",
                        params![prop_slug],
                        |_| Ok(()),
                    )
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => SchemaError::BatchItemError {
                            index: i,
                            source: Box::new(SchemaError::PropertyNotFound(
                                prop_slug.to_string(),
                            )),
                        },
                        other => SchemaError::Db(other),
                    })?;
            }
        }

        let tx = self.conn.transaction()?;
        let mut results = Vec::with_capacity(items.len());

        for (i, item) in items.iter().enumerate() {
            let id = Uuid::now_v7();
            let now = Utc::now();
            let created_at_str = now.to_rfc3339();

            tx.execute(
                "INSERT INTO entity_types (id, slug, description, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id.as_bytes().as_slice(), item.slug, item.description, created_at_str],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    SchemaError::BatchItemError {
                        index: i,
                        source: Box::new(SchemaError::DuplicateEntityType(
                            item.slug.to_string(),
                        )),
                    }
                }
                other => SchemaError::Db(other),
            })?;

            for prop_slug in item.properties {
                let prop_id: Vec<u8> = tx
                    .query_row(
                        "SELECT id FROM entity_properties WHERE slug = ?1",
                        params![prop_slug],
                        |row| row.get(0),
                    )
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => SchemaError::BatchItemError {
                            index: i,
                            source: Box::new(SchemaError::PropertyNotFound(
                                prop_slug.to_string(),
                            )),
                        },
                        other => SchemaError::Db(other),
                    })?;

                tx.execute(
                    "INSERT OR IGNORE INTO entity_type_properties (entity_type_id, entity_property_id) VALUES (?1, ?2)",
                    params![id.as_bytes().as_slice(), prop_id],
                )?;
            }

            results.push(EntityType {
                id,
                slug: item.slug.to_string(),
                description: item.description.map(String::from),
                created_at: now,
            });
        }

        tx.commit()?;
        Ok(results)
    }

    /// Creates properties in a single transaction with optional entity type attachment. All-or-nothing.
    pub fn create_properties_batch(
        &mut self,
        items: &[PropertyInput<'_>],
    ) -> Result<Vec<EntityProperty>, SchemaError> {
        for (i, item) in items.iter().enumerate() {
            validate_slug(item.slug).map_err(|e| SchemaError::BatchItemError {
                index: i,
                source: Box::new(e),
            })?;
            if reserved::is_reserved(item.slug) {
                return Err(SchemaError::BatchItemError {
                    index: i,
                    source: Box::new(SchemaError::ReservedProperty(item.slug.to_string())),
                });
            }
            for et_slug in item.entity_types {
                self.conn
                    .query_row(
                        "SELECT 1 FROM entity_types WHERE slug = ?1",
                        params![et_slug],
                        |_| Ok(()),
                    )
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => SchemaError::BatchItemError {
                            index: i,
                            source: Box::new(SchemaError::EntityTypeNotFound(
                                et_slug.to_string(),
                            )),
                        },
                        other => SchemaError::Db(other),
                    })?;
            }
        }

        let tx = self.conn.transaction()?;
        let mut results = Vec::with_capacity(items.len());

        for (i, item) in items.iter().enumerate() {
            let id = Uuid::now_v7();
            let now = Utc::now();
            let created_at_str = now.to_rfc3339();

            tx.execute(
                "INSERT INTO entity_properties (id, slug, description, value_type, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id.as_bytes().as_slice(), item.slug, item.description, item.value_type.as_str(), created_at_str],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(err, _)
                    if err.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    SchemaError::BatchItemError {
                        index: i,
                        source: Box::new(SchemaError::DuplicateProperty(
                            item.slug.to_string(),
                        )),
                    }
                }
                other => SchemaError::Db(other),
            })?;

            for et_slug in item.entity_types {
                let type_id: Vec<u8> = tx
                    .query_row(
                        "SELECT id FROM entity_types WHERE slug = ?1",
                        params![et_slug],
                        |row| row.get(0),
                    )
                    .map_err(|e| match e {
                        rusqlite::Error::QueryReturnedNoRows => SchemaError::BatchItemError {
                            index: i,
                            source: Box::new(SchemaError::EntityTypeNotFound(
                                et_slug.to_string(),
                            )),
                        },
                        other => SchemaError::Db(other),
                    })?;

                tx.execute(
                    "INSERT OR IGNORE INTO entity_type_properties (entity_type_id, entity_property_id) VALUES (?1, ?2)",
                    params![type_id, id.as_bytes().as_slice()],
                )?;
            }

            results.push(EntityProperty {
                id,
                slug: item.slug.to_string(),
                description: item.description.map(String::from),
                value_type: item.value_type,
                created_at: now,
            });
        }

        tx.commit()?;
        Ok(results)
    }

    /// Lists properties attached to a given entity type, ordered by slug.
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

        self.query_properties_by_type_id_bytes(&type_id)
    }

    /// Lists properties attached to an entity type identified by UUID.
    pub fn list_properties_by_type_id(
        &self,
        entity_type_id: &Uuid,
    ) -> Result<Vec<EntityProperty>, SchemaError> {
        let type_id_bytes = entity_type_id.as_bytes().to_vec();

        let exists: bool = self
            .conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM entity_types WHERE id = ?1)",
                params![type_id_bytes],
                |row| row.get(0),
            )
            .map_err(SchemaError::Db)?;

        if !exists {
            return Err(SchemaError::EntityTypeNotFound(entity_type_id.to_string()));
        }

        self.query_properties_by_type_id_bytes(&type_id_bytes)
    }

    fn query_properties_by_type_id_bytes(
        &self,
        type_id_bytes: &[u8],
    ) -> Result<Vec<EntityProperty>, SchemaError> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.slug, p.description, p.value_type, p.created_at
             FROM entity_properties p
             JOIN entity_type_properties tp ON tp.entity_property_id = p.id
             WHERE tp.entity_type_id = ?1
             ORDER BY p.slug",
        )?;

        let props = stmt
            .query_map(params![type_id_bytes], |row| {
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
        let et = reg.create_entity_type("research-project", None).unwrap();
        assert_eq!(et.slug, "research-project");
        assert!(et.description.is_none());
        assert_eq!(et.id.get_version(), Some(uuid::Version::SortRand));
    }

    #[test]
    fn create_entity_type_with_description() {
        let reg = registry();
        let et = reg
            .create_entity_type("sensor", Some("Data collection device"))
            .unwrap();
        assert_eq!(et.slug, "sensor");
        assert_eq!(et.description.as_deref(), Some("Data collection device"));
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
        reg.create_entity_type("research-project", None).unwrap();
        let prop = reg
            .create_property("project-name", ValueType::Struct, None)
            .unwrap();
        assert_eq!(prop.slug, "project-name");
        assert_eq!(prop.value_type, ValueType::Struct);

        reg.attach_property("research-project", "project-name").unwrap();

        let props = reg.list_properties("research-project").unwrap();
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].slug, "project-name");
        assert_eq!(props[0].value_type, ValueType::Struct);
    }

    #[test]
    fn create_property_with_description() {
        let reg = registry();
        let prop = reg
            .create_property("priority", ValueType::Struct, Some("Task priority level"))
            .unwrap();
        assert_eq!(prop.description.as_deref(), Some("Task priority level"));
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
        let created = reg.create_entity_type("sensor", None).unwrap();
        let fetched = reg.get_entity_type("sensor").unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.slug, "sensor");
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
        reg.create_entity_type("sensor", Some("Data collection device")).unwrap();
        let fetched = reg.get_entity_type("sensor").unwrap();
        assert_eq!(fetched.description.as_deref(), Some("Data collection device"));
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
    fn batch_create_entity_types() {
        let mut reg = registry();
        let items = vec![
            EntityTypeInput { slug: "alpha", description: None, properties: &[] },
            EntityTypeInput { slug: "bravo", description: Some("Second"), properties: &[] },
        ];
        let results = reg.create_entity_types_batch(&items).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].slug, "alpha");
        assert_eq!(results[1].slug, "bravo");
        assert_eq!(results[1].description.as_deref(), Some("Second"));

        let all = reg.list_entity_types().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn batch_create_entity_types_with_property_attachment() {
        let mut reg = registry();
        reg.create_property("name", ValueType::Struct, None).unwrap();
        reg.create_property("code", ValueType::Range, None).unwrap();

        let items = vec![
            EntityTypeInput { slug: "unit", description: None, properties: &["name", "code"] },
            EntityTypeInput { slug: "location", description: None, properties: &["name"] },
        ];
        let results = reg.create_entity_types_batch(&items).unwrap();
        assert_eq!(results.len(), 2);

        let unit_props = reg.list_properties("unit").unwrap();
        assert_eq!(unit_props.len(), 2);

        let loc_props = reg.list_properties("location").unwrap();
        assert_eq!(loc_props.len(), 1);
        assert_eq!(loc_props[0].slug, "name");
    }

    #[test]
    fn batch_create_entity_types_rollback_on_duplicate() {
        let mut reg = registry();
        reg.create_entity_type("existing", None).unwrap();

        let items = vec![
            EntityTypeInput { slug: "new-one", description: None, properties: &[] },
            EntityTypeInput { slug: "existing", description: None, properties: &[] },
        ];
        let err = reg.create_entity_types_batch(&items).unwrap_err();
        assert!(matches!(err, SchemaError::BatchItemError { index: 1, .. }));

        // new-one should not exist due to rollback
        assert!(reg.get_entity_type("new-one").is_err());
    }

    #[test]
    fn batch_create_entity_types_rollback_on_invalid_slug() {
        let mut reg = registry();
        let items = vec![
            EntityTypeInput { slug: "good", description: None, properties: &[] },
            EntityTypeInput { slug: "BAD", description: None, properties: &[] },
        ];
        let err = reg.create_entity_types_batch(&items).unwrap_err();
        assert!(matches!(err, SchemaError::BatchItemError { index: 1, .. }));

        assert!(reg.get_entity_type("good").is_err());
    }

    #[test]
    fn batch_create_entity_types_nonexistent_property() {
        let mut reg = registry();
        let items = vec![
            EntityTypeInput { slug: "unit", description: None, properties: &["no-such-prop"] },
        ];
        let err = reg.create_entity_types_batch(&items).unwrap_err();
        assert!(matches!(
            err,
            SchemaError::BatchItemError { index: 0, ref source }
            if matches!(source.as_ref(), SchemaError::PropertyNotFound(_))
        ));
    }

    #[test]
    fn batch_create_properties() {
        let mut reg = registry();
        let items = vec![
            PropertyInput { slug: "name", value_type: ValueType::Struct, description: None, entity_types: &[] },
            PropertyInput { slug: "count", value_type: ValueType::Range, description: Some("Amount"), entity_types: &[] },
        ];
        let results = reg.create_properties_batch(&items).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].slug, "name");
        assert_eq!(results[1].slug, "count");
        assert_eq!(results[1].description.as_deref(), Some("Amount"));

        let all = reg.list_all_properties().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn batch_create_properties_with_entity_type_attachment() {
        let mut reg = registry();
        reg.create_entity_type("unit", None).unwrap();
        reg.create_entity_type("location", None).unwrap();

        let items = vec![
            PropertyInput { slug: "name", value_type: ValueType::Struct, description: None, entity_types: &["unit", "location"] },
            PropertyInput { slug: "code", value_type: ValueType::Range, description: None, entity_types: &["unit"] },
        ];
        let results = reg.create_properties_batch(&items).unwrap();
        assert_eq!(results.len(), 2);

        let unit_props = reg.list_properties("unit").unwrap();
        assert_eq!(unit_props.len(), 2);

        let loc_props = reg.list_properties("location").unwrap();
        assert_eq!(loc_props.len(), 1);
        assert_eq!(loc_props[0].slug, "name");
    }

    #[test]
    fn batch_create_properties_rollback_on_reserved() {
        let mut reg = registry();
        let items = vec![
            PropertyInput { slug: "good-prop", value_type: ValueType::Struct, description: None, entity_types: &[] },
            PropertyInput { slug: "entity-uuid", value_type: ValueType::Struct, description: None, entity_types: &[] },
        ];
        let err = reg.create_properties_batch(&items).unwrap_err();
        assert!(matches!(
            err,
            SchemaError::BatchItemError { index: 1, ref source }
            if matches!(source.as_ref(), SchemaError::ReservedProperty(_))
        ));

        let all = reg.list_all_properties().unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn batch_create_properties_nonexistent_entity_type() {
        let mut reg = registry();
        let items = vec![
            PropertyInput { slug: "name", value_type: ValueType::Struct, description: None, entity_types: &["no-such-type"] },
        ];
        let err = reg.create_properties_batch(&items).unwrap_err();
        assert!(matches!(
            err,
            SchemaError::BatchItemError { index: 0, ref source }
            if matches!(source.as_ref(), SchemaError::EntityTypeNotFound(_))
        ));
    }

    #[test]
    fn batch_attach_properties() {
        let mut reg = registry();
        reg.create_entity_type("unit", None).unwrap();
        reg.create_entity_type("location", None).unwrap();
        reg.create_property("name", ValueType::Struct, None).unwrap();
        reg.create_property("code", ValueType::Range, None).unwrap();

        let items = vec![
            AttachInput { entity_type: "unit", property: "name" },
            AttachInput { entity_type: "unit", property: "code" },
            AttachInput { entity_type: "location", property: "name" },
        ];
        let count = reg.attach_properties_batch(&items).unwrap();
        assert_eq!(count, 3);

        let unit_props = reg.list_properties("unit").unwrap();
        assert_eq!(unit_props.len(), 2);

        let loc_props = reg.list_properties("location").unwrap();
        assert_eq!(loc_props.len(), 1);
    }

    #[test]
    fn batch_attach_nonexistent_entity_type() {
        let mut reg = registry();
        reg.create_property("name", ValueType::Struct, None).unwrap();

        let items = vec![
            AttachInput { entity_type: "no-such-type", property: "name" },
        ];
        let err = reg.attach_properties_batch(&items).unwrap_err();
        assert!(matches!(
            err,
            SchemaError::BatchItemError { index: 0, ref source }
            if matches!(source.as_ref(), SchemaError::EntityTypeNotFound(_))
        ));
    }

    #[test]
    fn batch_attach_nonexistent_property() {
        let mut reg = registry();
        reg.create_entity_type("unit", None).unwrap();

        let items = vec![
            AttachInput { entity_type: "unit", property: "no-such-prop" },
        ];
        let err = reg.attach_properties_batch(&items).unwrap_err();
        assert!(matches!(
            err,
            SchemaError::BatchItemError { index: 0, ref source }
            if matches!(source.as_ref(), SchemaError::PropertyNotFound(_))
        ));
    }

    #[test]
    fn batch_attach_rollback_on_error() {
        let mut reg = registry();
        reg.create_entity_type("unit", None).unwrap();
        reg.create_property("name", ValueType::Struct, None).unwrap();

        let items = vec![
            AttachInput { entity_type: "unit", property: "name" },
            AttachInput { entity_type: "unit", property: "no-such-prop" },
        ];
        let err = reg.attach_properties_batch(&items).unwrap_err();
        assert!(matches!(err, SchemaError::BatchItemError { index: 1, .. }));

        // First attachment should be rolled back
        let props = reg.list_properties("unit").unwrap();
        assert!(props.is_empty());
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
