use base64::Engine;
use log;
use rusqlite::types::{ToSql, ValueRef};
use rusqlite::{Connection, Result};
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tauri::command;
use tauri::{AppHandle, Manager};

pub struct DatabaseState {
    pub db_path: PathBuf,
}

/// Helper to log SQL in debug mode
pub fn log_sql(sql: &str) {
    log::debug!("SQL: {}", sql);
}

/// Initialize the database and ensure the file exists
pub fn init_db(app_handle: &AppHandle) -> Result<()> {
    let app_dir = app_handle
        .path()
        .app_data_dir()
        .expect("Failed to get app data dir");

    // Ensure app directory exists
    if !app_dir.exists() {
        fs::create_dir_all(&app_dir).expect("Failed to create app data directory");
    }

    let db_path = app_dir.join("overdone.db");

    // This will create the file if it doesn't exist
    let conn = Connection::open(&db_path)?;

    // Create settings table if it doesn't exist
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;

    // Create widget_state table
    conn.execute(
        "CREATE TABLE IF NOT EXISTS widget_state (
            id TEXT PRIMARY KEY,
            enabled BOOLEAN NOT NULL DEFAULT 0,
            config TEXT -- JSON blob for extra config
        )",
        [],
    )?;

    // Create file_tray table
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

/// Get a connection to the database
pub fn get_connection(app_handle: &AppHandle) -> Result<Connection> {
    let app_dir = app_handle.path().app_data_dir().unwrap();
    let db_path = app_dir.join("overdone.db");
    Connection::open(db_path)
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

#[command]
pub fn db_execute(
    app_handle: AppHandle,
    sql: String,
    args: Vec<JsonValue>,
) -> Result<usize, String> {
    log_sql(&sql);
    let conn = get_connection(&app_handle).map_err(|e| e.to_string())?;

    let sql_args: Vec<Box<dyn ToSql>> = args.iter().map(json_to_sql).collect();
    let sql_args_refs: Vec<&dyn ToSql> = sql_args.iter().map(|a| a.as_ref()).collect();

    conn.execute(&sql, sql_args_refs.as_slice())
        .map_err(|e| e.to_string())
}

#[command]
pub fn db_select(
    app_handle: AppHandle,
    sql: String,
    args: Vec<JsonValue>,
) -> Result<Vec<HashMap<String, JsonValue>>, String> {
    log_sql(&sql);
    let conn = get_connection(&app_handle).map_err(|e| e.to_string())?;

    let sql_args: Vec<Box<dyn ToSql>> = args.iter().map(json_to_sql).collect();
    let sql_args_refs: Vec<&dyn ToSql> = sql_args.iter().map(|a| a.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;

    // Get column names to create the hashmap
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
                    ValueRef::Blob(b) => {
                        JsonValue::String(base64::engine::general_purpose::STANDARD.encode(b))
                    }
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
