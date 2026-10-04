use serde::Serialize;
use std::{fs, path::{Path, PathBuf}, process::Command, sync::Mutex};

#[derive(Default)]
struct AppState(Mutex<Option<PathBuf>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    name: String,
    path: String,
    is_directory: bool,
    size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryListing {
    path: String,
    parent: Option<String>,
    entries: Vec<FileEntry>,
}

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "wmv", "flv", "mpg", "mpeg", "mts", "m2ts", "ts", "3gp", "vob", "ogv", "mxf",
];

fn video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| VIDEO_EXTENSIONS.iter().any(|item| item.eq_ignore_ascii_case(extension)))
}

fn selected_root(state: &AppState) -> Result<PathBuf, String> {
    state.0.lock().map_err(|_| "App state is unavailable".to_string())?
        .clone().ok_or_else(|| "Choose a folder first".to_string())
}

fn within_root(path: &str, state: &AppState) -> Result<PathBuf, String> {
    let root = selected_root(state)?;
    let resolved = Path::new(path).canonicalize().map_err(|error| format!("Cannot open path: {error}"))?;
    if !resolved.starts_with(&root) {
        return Err("This path is outside the selected folder".into());
    }
    Ok(resolved)
}

#[tauri::command]
fn select_root(path: String, state: tauri::State<'_, AppState>) -> Result<DirectoryListing, String> {
    let root = Path::new(&path).canonicalize().map_err(|error| format!("Cannot open folder: {error}"))?;
    if !root.is_dir() {
        return Err("The selected path is not a folder".into());
    }
    *state.0.lock().map_err(|_| "App state is unavailable".to_string())? = Some(root.clone());
    list_folder(&root, &root)
}

fn list_folder(path: &Path, root: &Path) -> Result<DirectoryListing, String> {
    let mut entries = Vec::new();
    for item in fs::read_dir(path).map_err(|error| format!("Cannot read folder: {error}"))? {
        let item = item.map_err(|error| format!("Cannot read folder entry: {error}"))?;
        let file_type = item.file_type().map_err(|error| format!("Cannot read file type: {error}"))?;
        // Avoid following links outside the selected folder.
        if file_type.is_symlink() || !(file_type.is_dir() || (file_type.is_file() && video_file(&item.path()))) {
            continue;
        }
        let path = item.path();
        entries.push(FileEntry {
            name: item.file_name().to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            is_directory: file_type.is_dir(),
            size: if file_type.is_file() { item.metadata().ok().map(|metadata| metadata.len()) } else { None },
        });
    }
    entries.sort_by(|a, b| b.is_directory.cmp(&a.is_directory).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(DirectoryListing {
        path: path.to_string_lossy().into_owned(),
        parent: if path == root { None } else { path.parent().map(|parent| parent.to_string_lossy().into_owned()) },
        entries,
    })
}

#[tauri::command]
fn list_directory(path: String, state: tauri::State<'_, AppState>) -> Result<DirectoryListing, String> {
    let root = selected_root(&state)?;
    let resolved = within_root(&path, &state)?;
    if !resolved.is_dir() {
        return Err("The selected path is not a folder".into());
    }
    list_folder(&resolved, &root)
}

#[tauri::command]
fn inspect_video(path: String, state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let resolved = within_root(&path, &state)?;
    if !resolved.is_file() || !video_file(&resolved) {
        return Err("Select a supported video file".into());
    }
    let probe = std::env::var_os("FFPROBE_PATH").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("ffprobe"));
    let output = Command::new(probe)
        .args(["-v", "error", "-show_format", "-show_streams", "-show_chapters", "-of", "json"])
        .arg(&resolved)
        .output()
        .map_err(|error| if error.kind() == std::io::ErrorKind::NotFound {
            "ffprobe was not found. Install FFmpeg and add ffprobe to PATH, or set FFPROBE_PATH.".to_string()
        } else { format!("Cannot start ffprobe: {error}") })?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Could not inspect video: {}", details.trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("Invalid ffprobe output: {error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_window_state::Builder::default().build())?;
            Ok(())
        })
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![select_root, list_directory, inspect_video])
        .run(tauri::generate_context!())
        .expect("error while building Tauri application");
}
