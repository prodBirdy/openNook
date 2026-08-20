use crate::app_data_dir;
use crate::utils::base64_encode;
use rusqlite::types::{ToSql, ValueRef};
use rusqlite::{Connection, Result};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::PathBuf;

pub fn log_sql(sql: &str) {
    log::debug!("SQL: {}", sql);
}

pub fn db_path() -> PathBuf {
    app_data_dir().join("opennook.db")
}

pub fn init_db() -> Result<()> {
    let conn = Connection::open(db_path())?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS widget_state (
            id TEXT PRIMARY KEY,
            enabled BOOLEAN NOT NULL DEFAULT 0,
            config TEXT
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS file_tray (
            path TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            size INTEGER,
            mime_type TEXT,
            last_modified INTEGER
        )",
        [],
    )?;

    Ok(())
}

pub fn get_connection() -> Result<Connection> {
    Connection::open(db_path())
}

fn json_to_sql(v: &JsonValue) -> Box<dyn ToSql> {
    match v {
        JsonValue::Null => Box::new(rusqlite::types::Null),
        JsonValue::Bool(b) => Box::new(*b),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else if let Some(f) = n.as_f64() {
                Box::new(f)
            } else {
                Box::new(n.to_string())
            }
        }
        JsonValue::String(s) => Box::new(s.clone()),
        JsonValue::Array(_) | JsonValue::Object(_) => Box::new(v.to_string()),
    }
}

pub fn db_execute(sql: String, args: Vec<JsonValue>) -> Result<usize, String> {
    log_sql(&sql);
    let conn = get_connection().map_err(|e| e.to_string())?;
    let sql_args: Vec<Box<dyn ToSql>> = args.iter().map(json_to_sql).collect();
    let sql_args_refs: Vec<&dyn ToSql> = sql_args.iter().map(|a| a.as_ref()).collect();
    conn.execute(&sql, sql_args_refs.as_slice())
        .map_err(|e| e.to_string())
}

pub fn db_select(
    sql: String,
    args: Vec<JsonValue>,
) -> Result<Vec<HashMap<String, JsonValue>>, String> {
    log_sql(&sql);
    let conn = get_connection().map_err(|e| e.to_string())?;
    let sql_args: Vec<Box<dyn ToSql>> = args.iter().map(json_to_sql).collect();
    let sql_args_refs: Vec<&dyn ToSql> = sql_args.iter().map(|a| a.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let col_names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let rows = stmt
        .query_map(sql_args_refs.as_slice(), |row| {
            let mut map = HashMap::new();
            for (i, col_name) in col_names.iter().enumerate() {
                let val = row.get_ref(i)?;
                let json_val = match val {
                    ValueRef::Null => JsonValue::Null,
                    ValueRef::Integer(i) => JsonValue::Number(serde_json::Number::from(i)),
                    ValueRef::Real(f) => serde_json::Number::from_f64(f)
                        .map(JsonValue::Number)
                        .unwrap_or(JsonValue::Null),
                    ValueRef::Text(t) => JsonValue::String(String::from_utf8_lossy(t).to_string()),
                    ValueRef::Blob(b) => JsonValue::String(base64_encode(b)),
                };
                map.insert(col_name.clone(), json_val);
            }
            Ok(map)
        })
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| e.to_string())?);
    }
    Ok(results)
}

pub fn get_setting(key: &str) -> Option<String> {
    let conn = get_connection().ok()?;
    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .ok()?;
    stmt.query_row([key], |row| row.get(0)).ok()
}

pub fn set_setting(key: &str, value: &str) -> Result<(), String> {
    let conn = get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
