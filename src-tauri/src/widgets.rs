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

#[command]
pub async fn run_speed_test(app_handle: AppHandle) -> Result<f64, String> {
    use futures_util::StreamExt;
    use std::time::Instant;
    use tauri::Emitter;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36")
        .build()
        .map_err(|e| e.to_string())?;

    // Use Cloudflare's speed test infrastructure
    // These are publicly available test files from Cloudflare
    let test_urls = vec![
        "https://speed.cloudflare.com/__down?bytes=25000000", // 25MB
        "https://proof.ovh.net/files/100Mb.dat",              // Alternative
    ];

    log::debug!("Starting speed test...");

    for test_url in test_urls {
        log::debug!("Testing with: {}", test_url);

        match client.get(test_url).send().await {
            Ok(response) => {
                let mut stream = response.bytes_stream();
                let start = Instant::now();
                let mut total_bytes = 0u64;
                let mut sample_count = 0u32;
                let mut last_sample_time = start;
                let max_duration = 8.0; // Maximum test duration in seconds

                while let Some(chunk_result) = stream.next().await {
                    match chunk_result {
                        Ok(chunk) => {
                            total_bytes += chunk.len() as u64;
                            sample_count += 1;

                            // Calculate speed every 100ms for smooth updates
                            if last_sample_time.elapsed().as_millis() >= 100 {
                                let elapsed = start.elapsed().as_secs_f64();
                                let bps = (total_bytes as f64 * 8.0) / elapsed;
                                let mbps = bps / 1_000_000.0;
                                last_sample_time = Instant::now();

                                // Calculate progress: 0% at 0s, 100% at 8s
                                let progress = ((elapsed / max_duration) * 100.0).min(100.0);

                                // Emit both speed and progress to UI
                                let _ = app_handle.emit(
                                    "speed_test_progress",
                                    serde_json::json!({
                                        "speed": mbps,
                                        "progress": progress
                                    }),
                                );

                                log::debug!(
                                    "Sample {}: {:.2} Mbps ({} bytes in {:.2}s) - {}% progress",
                                    sample_count,
                                    mbps,
                                    total_bytes,
                                    elapsed,
                                    progress as u32
                                );
                            }

                            // Stop after exactly 8 seconds
                            if start.elapsed().as_secs() >= 8 {
                                log::debug!("Stopping after 8 seconds");
                                break;
                            }
                        }
                        Err(e) => {
                            log::debug!("Stream error (may be expected): {}", e);
                            break;
                        }
                    }
                }

                let elapsed = start.elapsed().as_secs_f64();

                // Need at least 1 second of data for reliable measurement
                if elapsed < 1.0 {
                    log::debug!("Test too short: {:.2}s, trying next URL", elapsed);
                    continue;
                }

                // Need at least 1MB of data
                if total_bytes < 1024 * 1024 {
                    log::debug!(
                        "Not enough data downloaded: {} bytes, trying next URL",
                        total_bytes
                    );
                    continue;
                }

                // Calculate final speed
                let bps = (total_bytes as f64 * 8.0) / elapsed;
                let mbps = bps / 1_000_000.0;

                // Emit final 100% progress only when we're sure we have a valid result
                let _ = app_handle.emit(
                    "speed_test_progress",
                    serde_json::json!({
                        "speed": mbps,
                        "progress": 100.0
                    }),
                );

                log::debug!(
                    "Speed test complete: {:.2} Mbps ({} bytes in {:.2}s)",
                    mbps,
                    total_bytes,
                    elapsed
                );

                // Round to 2 decimal places
                return Ok((mbps * 100.0).round() / 100.0);
            }
            Err(e) => {
                log::debug!("Failed to connect to {}: {}", test_url, e);
                continue;
            }
        }
    }

    Err("All speed test servers failed. Please check your internet connection.".to_string())
}
