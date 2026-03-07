pub fn hello() -> &'static str {
    "terra incognita"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert_eq!(hello(), "terra incognita");
    }

    #[test]
    fn rocksdb_smoke() {
        let dir = tempfile::tempdir().unwrap();
        let db = rocksdb::DB::open_default(dir.path()).unwrap();

        db.put(b"entity:1", b"Moscow").unwrap();
        db.put(b"entity:2", b"Tula").unwrap();

        assert_eq!(db.get(b"entity:1").unwrap().unwrap(), b"Moscow");
        assert_eq!(db.get(b"entity:2").unwrap().unwrap(), b"Tula");

        db.delete(b"entity:2").unwrap();
        assert!(db.get(b"entity:2").unwrap().is_none());
    }

    #[test]
    fn rusqlite_smoke() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE entity_types (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
        )
        .unwrap();

        conn.execute(
            "INSERT INTO entity_types (id, name) VALUES (?1, ?2)",
            rusqlite::params!["mil_unit", "Military Unit"],
        )
        .unwrap();

        let name: String = conn
            .query_row(
                "SELECT name FROM entity_types WHERE id = ?1",
                rusqlite::params!["mil_unit"],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(name, "Military Unit");
    }
}
