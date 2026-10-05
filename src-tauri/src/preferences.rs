use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

use serde::{Deserialize, Serialize};
use tauri::Manager;

const SETTINGS_FILE: &str = "settings.json";
const WINDOW_STATE_FILE: &str = "window-state.json";
const SETTINGS_KEYS: &[&str] = &[
    "framewise.defaultFolder",
    "framewise.theme",
    "framewise.mediaView",
    "framewise.mediaSort",
    "framewise.sortDirection",
    "framewise.workspaceCollapsed",
    "framewise.inspectorCollapsed",
    "framewise.workspaceWidth",
    "framewise.inspectorWidth",
    "framewise.settingsUpdatedAt",
];

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    values: BTreeMap<String, String>,
}

pub(super) fn settings_dir() -> Result<PathBuf, String> {
    dirs::home_dir()
        .map(|home| home.join(".framewise"))
        .ok_or_else(|| "Cannot find your home folder for Framewise settings".into())
}

pub(super) fn window_state_path() -> Result<PathBuf, String> {
    Ok(settings_dir()?.join(WINDOW_STATE_FILE))
}

fn copy_if_missing(old: &Path, target: &Path) -> Result<(), String> {
    if target.exists() || !old.is_file() {
        return Ok(());
    }
    fs::create_dir_all(target.parent().ok_or("Cannot find settings folder")?)
        .map_err(|error| format!("Cannot create settings folder: {error}"))?;
    fs::copy(old, target).map_err(|error| format!("Cannot migrate settings: {error}"))?;
    Ok(())
}

#[cfg(desktop)]
pub(super) fn migrate_window_state() -> Result<(), String> {
    let target = window_state_path()?;
    fs::create_dir_all(target.parent().ok_or("Cannot find settings folder")?)
        .map_err(|error| format!("Cannot create settings folder: {error}"))?;
    let old = dirs::config_dir()
        .ok_or("Cannot find the previous window settings folder")?
        .join("app.framewise.desktop")
        .join(".window-state.json");
    copy_if_missing(&old, &target)
}

fn read_settings(path: &Path) -> Result<Option<BTreeMap<String, String>>, String> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot read settings: {error}")),
    };
    let saved: SettingsFile = serde_json::from_slice(&contents)
        .map_err(|error| format!("Cannot read saved settings: {error}"))?;
    if saved.version != 1 {
        return Err(format!("Unsupported settings version: {}", saved.version));
    }
    Ok(Some(saved.values))
}

fn write_settings(path: &Path, values: BTreeMap<String, String>) -> Result<(), String> {
    if values.iter().any(|(key, value)| !SETTINGS_KEYS.contains(&key.as_str()) || value.len() > 32_768) {
        return Err("Invalid settings value".into());
    }
    let contents = serde_json::to_vec_pretty(&SettingsFile { version: 1, values })
        .map_err(|error| format!("Cannot encode settings: {error}"))?;
    let parent = path.parent().ok_or("Cannot find settings folder")?;
    fs::create_dir_all(parent).map_err(|error| format!("Cannot create settings folder: {error}"))?;
    fs::write(path, contents).map_err(|error| format!("Cannot save settings: {error}"))
}

#[tauri::command]
pub(super) fn load_preferences(app: tauri::AppHandle) -> Result<Option<BTreeMap<String, String>>, String> {
    let path = settings_dir()?.join(SETTINGS_FILE);
    if let Some(values) = read_settings(&path)? {
        return Ok(Some(values));
    }
    let old = app.path().app_config_dir()
        .map_err(|error| format!("Cannot find previous settings folder: {error}"))?
        .join("framewise-settings.json");
    if let Some(values) = read_settings(&old)? {
        write_settings(&path, values.clone())?;
        return Ok(Some(values));
    }
    Ok(None)
}

#[tauri::command]
pub(super) fn save_preferences(values: BTreeMap<String, String>) -> Result<(), String> {
    let path = settings_dir()?.join(SETTINGS_FILE);
    write_settings(&path, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_removal() {
        let folder = std::env::temp_dir().join(format!("framewise-preferences-{}", std::process::id()));
        let path = folder.join(SETTINGS_FILE);
        let mut values = BTreeMap::new();
        values.insert("framewise.theme".into(), "dark".into());
        values.insert("framewise.mediaView".into(), "grid".into());
        values.insert("framewise.mediaSort".into(), "modified".into());
        values.insert("framewise.sortDirection".into(), "desc".into());
        values.insert("framewise.defaultFolder".into(), "C:\\media".into());
        write_settings(&path, values.clone()).unwrap();
        assert_eq!(read_settings(&path).unwrap(), Some(values));
        write_settings(&path, BTreeMap::new()).unwrap();
        assert_eq!(read_settings(&path).unwrap(), Some(BTreeMap::new()));
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn migration_preserves_existing_settings() {
        let folder = std::env::temp_dir().join(format!("framewise-settings-migration-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let old = folder.join("old.json");
        let target = folder.join(".framewise").join(WINDOW_STATE_FILE);
        fs::write(&old, "old state").unwrap();
        copy_if_missing(&old, &target).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "old state");
        fs::write(&target, "new state").unwrap();
        copy_if_missing(&old, &target).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new state");
        fs::remove_dir_all(folder).unwrap();
    }
}
