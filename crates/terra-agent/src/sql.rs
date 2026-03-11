use std::time::Instant;

use serde::Serialize;
use tokio::runtime::Runtime;
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

    match *col_type {
        Type::BOOL => try_get!(bool),
        Type::INT2 => try_get!(i16),
        Type::INT4 => try_get!(i32),
        Type::INT8 => try_get!(i64),
        Type::FLOAT4 => try_get!(f32),
        Type::FLOAT8 => try_get!(f64),
        Type::TEXT | Type::VARCHAR | Type::NAME | Type::BPCHAR => try_get!(String),
        Type::JSON | Type::JSONB => try_get!(serde_json::Value),
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
