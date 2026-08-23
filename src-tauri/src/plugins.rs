use log;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{command, AppHandle};

/// Plugin manifest as defined in plugin.json
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub main: String,
    pub category: String,
    #[serde(rename = "minWidth")]
    pub min_width: Option<u32>,
    #[serde(rename = "hasCompactMode")]
    pub has_compact_mode: bool,
    #[serde(rename = "compactPriority")]
    pub compact_priority: Option<u32>,
    pub permissions: Vec<String>,
}

/// Information about a discovered plugin
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PluginInfo {
    pub manifest: PluginManifest,
    pub bundle_path: String,
    pub plugin_dir: String,
}

/// Get the plugins directory path
fn get_plugins_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".opennook").join("plugins")
}

/// Scan the plugins directory and return information about all valid plugins
#[command]
pub fn scan_plugins_directory(_app_handle: AppHandle) -> Result<Vec<PluginInfo>, String> {
    let plugins_dir = get_plugins_dir();

    // Create directory if it doesn't exist
    if !plugins_dir.exists() {
        fs::create_dir_all(&plugins_dir).map_err(|e| e.to_string())?;
        return Ok(vec![]);
    }

    let mut plugins = Vec::new();

    // Read all entries in the plugins directory
    let entries = fs::read_dir(&plugins_dir).map_err(|e| e.to_string())?;

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();

        // Skip non-directories
        if !path.is_dir() {
            continue;
        }

        // Look for plugin.json
        let manifest_path = path.join("plugin.json");
        if !manifest_path.exists() {
            continue;
        }

        // Read and parse manifest
        let manifest_content = match fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!(
                    "Failed to read plugin manifest at {:?}: {}",
                    manifest_path,
                    e
                );
                continue;
            }
        };

        let manifest: PluginManifest = match serde_json::from_str(&manifest_content) {
            Ok(m) => m,
            Err(e) => {
                log::error!(
                    "Failed to parse plugin manifest at {:?}: {}",
                    manifest_path,
                    e
                );
                continue;
            }
        };

        // Verify the main bundle exists
        let bundle_path = path.join(&manifest.main);
        if !bundle_path.exists() {
            log::error!("Plugin bundle not found: {:?}", bundle_path);
            continue;
        }

        plugins.push(PluginInfo {
            manifest,
            bundle_path: bundle_path.to_string_lossy().to_string(),
            plugin_dir: path.to_string_lossy().to_string(),
        });
    }

    Ok(plugins)
}

/// Read the content of a plugin's JavaScript bundle
#[command]
pub fn read_plugin_bundle(_app_handle: AppHandle, bundle_path: String) -> Result<String, String> {
    fs::read_to_string(&bundle_path).map_err(|e| e.to_string())
}

/// Get the plugins directory path (for frontend use)
#[command]
pub fn get_plugins_directory_path() -> String {
    get_plugins_dir().to_string_lossy().to_string()
}

/// Validate a plugin folder has valid plugin.json and return its info
fn validate_plugin_folder(path: &PathBuf) -> Result<PluginInfo, String> {
    let manifest_path = path.join("plugin.json");
    if !manifest_path.exists() {
        return Err("plugin.json not found".to_string());
    }

    let manifest_content = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Failed to read plugin.json: {}", e))?;

    let manifest: PluginManifest = serde_json::from_str(&manifest_content)
        .map_err(|e| format!("Invalid plugin.json: {}", e))?;

    let bundle_path = path.join(&manifest.main);
    if !bundle_path.exists() {
        return Err(format!("Bundle file '{}' not found", manifest.main));
    }

    Ok(PluginInfo {
        manifest,
        bundle_path: bundle_path.to_string_lossy().to_string(),
        plugin_dir: path.to_string_lossy().to_string(),
    })
}

/// Install a plugin from a local folder (copies to plugins directory)
#[command]
pub fn install_plugin_from_folder(
    _app_handle: AppHandle,
    source_path: String,
) -> Result<PluginInfo, String> {
    let source = PathBuf::from(&source_path);

    if !source.is_dir() {
        return Err("Source path is not a directory".to_string());
    }

    // Validate source folder
    let plugin_info = validate_plugin_folder(&source)?;
    let plugin_id = &plugin_info.manifest.id;

    // Destination path
    let plugins_dir = get_plugins_dir();
    fs::create_dir_all(&plugins_dir).map_err(|e| e.to_string())?;

    let dest = plugins_dir.join(plugin_id);

    // Remove existing if present
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .map_err(|e| format!("Failed to remove existing plugin: {}", e))?;
    }

    // Copy directory recursively
    copy_dir_all(&source, &dest).map_err(|e| format!("Failed to copy plugin: {}", e))?;

    // Return info for the installed plugin
    validate_plugin_folder(&dest)
}

/// Recursively copy a directory
fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Install a plugin from a Git repository URL
#[command]
pub async fn install_plugin_from_git(
    _app_handle: AppHandle,
    repo_url: String,
) -> Result<PluginInfo, String> {
    use std::process::Command;

    // Create temp directory for cloning
    let temp_dir = std::env::temp_dir().join(format!(
        "opennook-plugin-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));

    // Clone the repository
    let output = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            &repo_url,
            &temp_dir.to_string_lossy(),
        ])
        .output()
        .map_err(|e| format!("Failed to run git: {}. Is git installed?", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Git clone failed: {}", stderr));
    }

    // Validate the cloned repo
    let plugin_info = validate_plugin_folder(&temp_dir)?;
    let plugin_id = plugin_info.manifest.id.clone();

    // Move to plugins directory
    let plugins_dir = get_plugins_dir();
    fs::create_dir_all(&plugins_dir).map_err(|e| e.to_string())?;

    let dest = plugins_dir.join(&plugin_id);

    // Remove existing if present
    if dest.exists() {
        fs::remove_dir_all(&dest)
            .map_err(|e| format!("Failed to remove existing plugin: {}", e))?;
    }

    // Move from temp to plugins dir
    fs::rename(&temp_dir, &dest)
        .or_else(|_| {
            // If rename fails (cross-device), copy instead
            copy_dir_all(&temp_dir, &dest)?;
            fs::remove_dir_all(&temp_dir)
        })
        .map_err(|e| format!("Failed to install plugin: {}", e))?;

    // Return info for the installed plugin
    validate_plugin_folder(&dest)
}

/// Delete an installed plugin
#[command]
pub fn delete_plugin(_app_handle: AppHandle, plugin_id: String) -> Result<(), String> {
    let plugins_dir = get_plugins_dir();
    let plugin_path = plugins_dir.join(&plugin_id);

    if !plugin_path.exists() {
        return Err(format!("Plugin '{}' not found", plugin_id));
    }

    if !plugin_path.is_dir() {
        return Err("Invalid plugin path".to_string());
    }

    // Safety check: ensure it's inside plugins directory
    if !plugin_path.starts_with(&plugins_dir) {
        return Err("Security error: path traversal detected".to_string());
    }

    fs::remove_dir_all(&plugin_path).map_err(|e| format!("Failed to delete plugin: {}", e))
}
