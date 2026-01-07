use crate::database::{get_connection, log_sql};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{command, AppHandle};

/// Widget enabled state - maps widget IDs to their enabled status
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct WidgetState {
    pub enabled: HashMap<String, bool>,
}

/// Save widget enabled state to disk (SQLite)
#[command]
pub fn save_widget_state(app_handle: AppHandle, state: WidgetState) -> Result<(), String> {
    let conn = get_connection(&app_handle).map_err(|e| e.to_string())?;

    // Transaction to update all widget states
    conn.execute_batch("BEGIN TRANSACTION;")
        .map_err(|e| e.to_string())?;

    for (id, enabled) in state.enabled {
        let sql = "INSERT OR REPLACE INTO widget_state (id, enabled) VALUES (?1, ?2)";
        log_sql(&format!("{} [{}, {}]", sql, id, enabled));

        conn.execute(sql, rusqlite::params![id, enabled])
            .map_err(|e| e.to_string())?;
    }

    conn.execute_batch("COMMIT;").map_err(|e| e.to_string())?;
    Ok(())
}

/// Load widget enabled state from disk (SQLite)
#[command]
pub fn load_widget_state(app_handle: AppHandle) -> Result<WidgetState, String> {
    let conn = get_connection(&app_handle).map_err(|e| e.to_string())?;

    let sql = "SELECT id, enabled FROM widget_state";
    log_sql(sql);

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?))
        })
        .map_err(|e| e.to_string())?;

    let mut enabled = HashMap::new();
    for row in rows {
        let (id, is_enabled) = row.map_err(|e| e.to_string())?;
        enabled.insert(id, is_enabled);
    }

    Ok(WidgetState { enabled })
}
