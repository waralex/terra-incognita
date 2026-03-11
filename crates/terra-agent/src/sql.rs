use std::time::Instant;

use serde::Serialize;
use tokio::runtime::Runtime;
use rust_decimal::prelude::ToPrimitive;
use tokio_postgres::types::Type;
use tokio_postgres::{Client, NoTls, Row};

const MAX_ROWS: usize = 100;
const MAX_RESULT_BYTES: usize = 50_000;

/// SQL query result returned to the caller.
#[derive(Serialize)]
pub struct SqlResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub row_count: usize,
    pub truncated: bool,
    pub elapsed_ms: u128,
}

/// Async PostgreSQL query tool.
///
/// Env vars:
/// - `TERRA_SQL_URL` — full connection string (overrides individual vars)
/// - `TERRA_SQL_HOST` (default: `localhost`)
/// - `TERRA_SQL_PORT` (default: `5442`)
/// - `TERRA_SQL_USER` (default: current OS user)
/// - `TERRA_SQL_PASSWORD`
/// - `TERRA_SQL_DATABASE`
pub struct SqlTool {
    connection_string: String,
    rt: Runtime,
    /// Database name for display/context purposes.
    pub database: String,
}

impl SqlTool {
    /// Creates a new SqlTool from environment variables.
    pub fn from_env() -> Result<Self, String> {
        let connection_string = if let Ok(url) = std::env::var("TERRA_SQL_URL") {
            url
        } else {
            let host = std::env::var("TERRA_SQL_HOST").unwrap_or_else(|_| "localhost".into());
            let port = std::env::var("TERRA_SQL_PORT").unwrap_or_else(|_| "5442".into());
            let user = std::env::var("TERRA_SQL_USER").unwrap_or_else(|_| whoami());
            let password = std::env::var("TERRA_SQL_PASSWORD").ok();
            let database = std::env::var("TERRA_SQL_DATABASE").ok();

            let mut s = format!("host={host} port={port} user={user}");
            if let Some(pw) = password {
                s.push_str(&format!(" password={pw}"));
            }
            if let Some(db) = database {
                s.push_str(&format!(" dbname={db}"));
            }
            s
        };

        let database = std::env::var("TERRA_SQL_DATABASE")
            .or_else(|_| std::env::var("TERRA_SQL_URL").map(|u| {
                // Try to extract dbname from URL
                u.rsplit('/').next().unwrap_or("unknown").to_string()
            }))
            .unwrap_or_else(|_| "unknown".into());

        let rt = Runtime::new().map_err(|e| format!("failed to create tokio runtime: {e}"))?;
        Ok(Self { connection_string, rt, database })
    }

    /// Executes a read-only SQL query and returns JSON results.
    /// Rejects statements containing mutating keywords.
    pub fn execute(&self, sql: &str) -> Result<SqlResult, String> {
        reject_mutating(sql)?;
        self.rt.block_on(self.execute_async(sql))
    }

    async fn execute_async(&self, sql: &str) -> Result<SqlResult, String> {
        let (client, connection) = tokio_postgres::connect(&self.connection_string, NoTls)
            .await
            .map_err(|e| format!("connection failed: {e}"))?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("postgres connection error: {e}");
            }
        });

        let start = Instant::now();
        let rows = client
            .query(sql, &[])
            .await
            .map_err(|e| format!("query failed: {e}"))?;
        let elapsed_ms = start.elapsed().as_millis();

        let total_rows = rows.len();
        let truncated = total_rows > MAX_ROWS;
        let rows_to_process = &rows[..total_rows.min(MAX_ROWS)];

        let columns = column_names(&client, rows_to_process);
        let json_rows = rows_to_json(rows_to_process);

        let result = SqlResult {
            columns,
            rows: json_rows,
            row_count: total_rows,
            truncated,
            elapsed_ms,
        };

        let serialized_size = serde_json::to_string(&result).map_err(|e| e.to_string())?.len();
        if serialized_size > MAX_RESULT_BYTES {
            return Err(format!(
                "result too large: {serialized_size} bytes, {total_rows} rows, {elapsed_ms}ms. \
                 Limit is {MAX_RESULT_BYTES} bytes. Use LIMIT or narrow your SELECT columns."
            ));
        }

        Ok(result)
    }
}

fn column_names(_client: &Client, rows: &[Row]) -> Vec<String> {
    rows.first()
        .map(|r| r.columns().iter().map(|c| c.name().to_string()).collect())
        .unwrap_or_default()
}

fn rows_to_json(rows: &[Row]) -> Vec<Vec<serde_json::Value>> {
    rows.iter().map(|row| row_to_json(row)).collect()
}

fn row_to_json(row: &Row) -> Vec<serde_json::Value> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(i, col)| column_to_json(row, i, col.type_()))
        .collect()
}

fn column_to_json(row: &Row, idx: usize, col_type: &Type) -> serde_json::Value {
    macro_rules! try_get {
        ($t:ty) => {
            match row.try_get::<_, Option<$t>>(idx) {
                Ok(Some(v)) => return serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
                Ok(None) => return serde_json::Value::Null,
                Err(_) => {}
            }
        };
    }

    macro_rules! try_get_str {
        ($t:ty) => {
            match row.try_get::<_, Option<$t>>(idx) {
                Ok(Some(v)) => return serde_json::Value::String(v.to_string()),
                Ok(None) => return serde_json::Value::Null,
                Err(_) => {}
            }
        };
    }

    match *col_type {
        // Booleans
        Type::BOOL => try_get!(bool),

        // Integers
        Type::INT2 => try_get!(i16),
        Type::INT4 | Type::OID => try_get!(i32),
        Type::INT8 => try_get!(i64),

        // Floats
        Type::FLOAT4 => try_get!(f32),
        Type::FLOAT8 => try_get!(f64),

        // Numeric/Decimal — no native Rust type without rust_decimal
        Type::NUMERIC => {
            match row.try_get::<_, Option<rust_decimal::Decimal>>(idx) {
                Ok(Some(d)) => {
                    if let Some(f) = d.to_f64() {
                        return serde_json::Value::from(f);
                    }
                    return serde_json::Value::String(d.to_string());
                }
                Ok(None) => return serde_json::Value::Null,
                Err(_) => {}
            }
        }

        // Strings
        Type::TEXT | Type::VARCHAR | Type::NAME | Type::BPCHAR | Type::CHAR => try_get!(String),

        // Date & time — serialize as ISO strings
        Type::DATE => try_get_str!(chrono::NaiveDate),
        Type::TIME => try_get_str!(chrono::NaiveTime),
        Type::TIMESTAMP => try_get_str!(chrono::NaiveDateTime),
        Type::TIMESTAMPTZ => try_get_str!(chrono::DateTime<chrono::Utc>),
        Type::INTERVAL => {
            // Interval has no chrono mapping; read raw fields
            match row.try_get::<_, Option<String>>(idx) {
                Ok(Some(s)) => return serde_json::Value::String(s),
                Ok(None) => return serde_json::Value::Null,
                Err(_) => {}
            }
        }

        // UUID
        Type::UUID => try_get_str!(uuid::Uuid),

        // JSON
        Type::JSON | Type::JSONB => try_get!(serde_json::Value),

        // Arrays of common types
        Type::BOOL_ARRAY => try_get!(Vec<bool>),
        Type::INT2_ARRAY => try_get!(Vec<i16>),
        Type::INT4_ARRAY => try_get!(Vec<i32>),
        Type::INT8_ARRAY => try_get!(Vec<i64>),
        Type::FLOAT4_ARRAY => try_get!(Vec<f32>),
        Type::FLOAT8_ARRAY => try_get!(Vec<f64>),
        Type::TEXT_ARRAY | Type::VARCHAR_ARRAY => try_get!(Vec<String>),

        // Bytea — hex-encode for JSON
        Type::BYTEA => {
            match row.try_get::<_, Option<Vec<u8>>>(idx) {
                Ok(Some(bytes)) => {
                    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    return serde_json::Value::String(format!("\\x{hex}"));
                }
                Ok(None) => return serde_json::Value::Null,
                Err(_) => {}
            }
        }

        _ => {}
    }

    // Fallback: try common types in order
    try_get!(String);
    try_get!(i64);
    try_get!(f64);
    try_get!(bool);

    serde_json::Value::Null
}

const FORBIDDEN_KEYWORDS: &[&str] = &[
    "INSERT", "UPDATE", "DELETE", "DROP", "ALTER", "CREATE", "TRUNCATE",
];

fn reject_mutating(sql: &str) -> Result<(), String> {
    let upper = sql.to_ascii_uppercase();
    for kw in FORBIDDEN_KEYWORDS {
        if upper.split_ascii_whitespace().any(|w| w == *kw) {
            return Err(format!("rejected: {kw} statements are not allowed (read-only mode)"));
        }
    }
    Ok(())
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "postgres".into())
}
