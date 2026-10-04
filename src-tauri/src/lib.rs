use serde::Serialize;
use std::{collections::{hash_map::DefaultHasher, HashSet}, fs, hash::{Hash, Hasher}, io, path::{Path, PathBuf}, process::{Command, Output}, sync::Mutex, time::UNIX_EPOCH};
use tauri::Manager;
mod editor;
mod duplicate;
mod rename;
mod move_files;
mod preferences;
#[cfg(desktop)]
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrashBatchResult {
    listing: DirectoryListing,
    moved_count: usize,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Preview {
    video_path: String,
    video_version: String,
    thumbnail_path: Option<String>,
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
async fn move_videos_to_trash(paths: Vec<String>, state: tauri::State<'_, AppState>) -> Result<TrashBatchResult, String> {
    if paths.is_empty() {
        return Err("Select at least one video".into());
    }
    let root = selected_root(&state)?;
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut parent: Option<PathBuf> = None;
    for path in paths {
        let file_type = fs::symlink_metadata(&path)
            .map_err(|error| format!("Cannot open video: {error}"))?
            .file_type();
        if !file_type.is_file() {
            return Err("Select regular video files".into());
        }
        let resolved = within_root(&path, &state)?;
        if !video_file(&resolved) {
            return Err("Select supported video files".into());
        }
        let folder = resolved.parent().ok_or("Cannot find the video's folder")?;
        if parent.as_deref().is_some_and(|current| current != folder) {
            return Err("Selected videos must be in the same folder".into());
        }
        parent = Some(folder.to_path_buf());
        if seen.insert(resolved.clone()) {
            files.push(resolved);
        }
    }
    let parent = parent.ok_or("Cannot find the videos' folder")?;
    tauri::async_runtime::spawn_blocking(move || {
        let error = trash::delete_all(&files).err().map(|cause| {
            log::error!("Could not move every video to Trash: {cause}");
            format!("Could not move every video to Trash: {cause}")
        });
        let moved_count = files.iter().filter(|path| {
            matches!(fs::symlink_metadata(path), Err(cause) if cause.kind() == io::ErrorKind::NotFound)
        }).count();
        let listing = list_folder(&parent, &root)?;
        Ok(TrashBatchResult { listing, moved_count, error })
    }).await.map_err(|error| format!("Trash operation failed: {error}"))?
}

#[tauri::command]
fn move_folder_to_trash(path: String, state: tauri::State<'_, AppState>) -> Result<DirectoryListing, String> {
    let file_type = fs::symlink_metadata(&path)
        .map_err(|error| format!("Cannot open folder: {error}"))?
        .file_type();
    if !file_type.is_dir() {
        return Err("Select a regular folder".into());
    }
    let root = selected_root(&state)?;
    let resolved = within_root(&path, &state)?;
    if resolved == root {
        return Err("The selected media folder cannot be moved to Trash.".into());
    }
    let parent = resolved.parent().ok_or("Cannot find the folder's parent")?;
    trash::delete(&resolved).map_err(|error| {
        log::error!("Could not move folder to Trash: {error}");
        format!("Could not move folder to Trash: {error}")
    })?;
    list_folder(parent, &root)
}

#[tauri::command]
fn inspect_video(path: String, state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let resolved = within_root(&path, &state)?;
    if !resolved.is_file() || !video_file(&resolved) {
        return Err("Select a supported video file".into());
    }
    let output = run_ffprobe(&resolved)
        .map_err(|error| {
            log::error!("ffprobe could not start: {error}");
            if error.kind() == io::ErrorKind::NotFound {
                "ffprobe was not found. Install FFmpeg and add ffprobe to PATH, or set FFPROBE_PATH.".to_string()
            } else { format!("Cannot start ffprobe: {error}") }
        })?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        log::error!("ffprobe failed: {}", details.trim());
        return Err(format!("Could not inspect video: {}", details.trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        log::error!("ffprobe returned invalid JSON: {error}");
        format!("Invalid ffprobe output: {error}")
    })
}

fn execute_ffprobe(probe: &Path, video: &Path, args: &[&str]) -> io::Result<Output> {
    Command::new(probe)
        .args(["-v", "error"])
        .args(args)
        .arg(video)
        .output()
}

fn run_ffprobe(video: &Path) -> io::Result<Output> {
    run_ffprobe_with_args(video, &["-show_format", "-show_streams", "-show_chapters", "-of", "json"])
}

fn run_ffprobe_with_args(video: &Path, args: &[&str]) -> io::Result<Output> {
    if let Some(configured) = std::env::var_os("FFPROBE_PATH") {
        return execute_ffprobe(Path::new(&configured), video, args);
    }
    match execute_ffprobe(Path::new("ffprobe"), video, args) {
        #[cfg(windows)]
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(probe) = winget_ffprobe() {
                log::info!("Using ffprobe from Windows Package Manager: {}", probe.display());
                execute_ffprobe(&probe, video, args)
            } else { Err(error) }
        }
        result => result,
    }
}

#[cfg(windows)]
fn winget_ffprobe() -> Option<PathBuf> {
    let packages = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
        .join("Microsoft").join("WinGet").join("Packages");
    for package in fs::read_dir(packages).ok()?.flatten() {
        if !package.file_name().to_string_lossy().to_ascii_lowercase().contains("ffmpeg") {
            continue;
        }
        let direct = package.path().join("bin").join("ffprobe.exe");
        if direct.is_file() { return Some(direct); }
        let Ok(installations) = fs::read_dir(package.path()) else { continue; };
        for installation in installations.flatten() {
            let candidate = installation.path().join("bin").join("ffprobe.exe");
            if candidate.is_file() { return Some(candidate); }
        }
    }
    None
}

fn execute_ffmpeg(program: &Path, video: &Path, thumbnail: &Path, seek: &str) -> io::Result<Output> {
    Command::new(program)
        .args(["-hide_banner", "-loglevel", "error", "-ss", seek, "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-vf", "scale=480:-2", "-q:v", "3", "-y"])
        .arg(thumbnail)
        .output()
}

fn run_ffmpeg(video: &Path, thumbnail: &Path, seek: &str) -> io::Result<Output> {
    if let Some(configured) = std::env::var_os("FFMPEG_PATH") {
        return execute_ffmpeg(Path::new(&configured), video, thumbnail, seek);
    }
    match execute_ffmpeg(Path::new("ffmpeg"), video, thumbnail, seek) {
        #[cfg(windows)]
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let sibling = std::env::var_os("FFPROBE_PATH")
                .map(PathBuf::from)
                .filter(|path| path.is_file())
                .map(|path| path.with_file_name("ffmpeg.exe"));
            let fallback = sibling.filter(|path| path.is_file())
                .or_else(|| winget_ffprobe().map(|path| path.with_file_name("ffmpeg.exe")));
            if let Some(program) = fallback.filter(|path| path.is_file()) {
                execute_ffmpeg(&program, video, thumbnail, seek)
            } else { Err(error) }
        }
        result => result,
    }
}

fn cached_thumbnail(video: &Path, app: &tauri::AppHandle) -> Option<PathBuf> {
    let metadata = fs::metadata(video).ok()?;
    let mut hash = DefaultHasher::new();
    video.hash(&mut hash);
    metadata.len().hash(&mut hash);
    metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos().hash(&mut hash);
    let directory = app.path().app_cache_dir().ok()?.join("previews");
    if let Err(error) = fs::create_dir_all(&directory) {
        log::warn!("Could not create preview cache: {error}");
        return None;
    }
    let thumbnail = directory.join(format!("{:016x}.jpg", hash.finish()));
    if thumbnail.metadata().is_ok_and(|file| file.len() > 0) {
        return Some(thumbnail);
    }
    for seek in ["1", "0"] {
        match run_ffmpeg(video, &thumbnail, seek) {
            Ok(output) if output.status.success() && thumbnail.metadata().is_ok_and(|file| file.len() > 0) => return Some(thumbnail),
            Ok(output) => log::warn!("Could not create video thumbnail: {}", String::from_utf8_lossy(&output.stderr).trim()),
            Err(error) => { log::warn!("Could not start ffmpeg for thumbnail: {error}"); break; }
        }
    }
    None
}

#[tauri::command]
fn prepare_preview(path: String, state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Result<Preview, String> {
    let video = within_root(&path, &state)?;
    if !video.is_file() || !video_file(&video) {
        return Err("Select a supported video file".into());
    }
    let video_version = editor::signature(&video)?;
    app.asset_protocol_scope().allow_file(&video).map_err(|error| format!("Cannot open video preview: {error}"))?;
    let thumbnail_path = cached_thumbnail(&video, &app).and_then(|thumbnail| {
        if let Err(error) = app.asset_protocol_scope().allow_file(&thumbnail) {
            log::warn!("Could not expose video thumbnail: {error}");
            None
        } else { Some(thumbnail.to_string_lossy().into_owned()) }
    });
    Ok(Preview { video_path: video.to_string_lossy().into_owned(), video_version, thumbnail_path })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new()
            .level(log::LevelFilter::Info)
            .max_file_size(1_000_000)
            .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(2))
            .build())
        .plugin(tauri_plugin_dialog::init());
    #[cfg(desktop)]
    let builder = {
        let mut window_state = tauri_plugin_window_state::Builder::default();
        if let Err(error) = preferences::migrate_window_state() {
            log::error!("Could not prepare persistent window settings: {error}");
        } else if let Ok(path) = preferences::window_state_path() {
            window_state = window_state.with_filename(path.to_string_lossy().into_owned());
        }
        builder.plugin(window_state.build())
    };
    #[cfg(desktop)]
    let builder = builder.on_window_event(|window, event| {
        if let tauri::WindowEvent::CloseRequested { .. } = event
            && let Err(error) = window.app_handle().save_window_state(StateFlags::all())
        {
            log::error!("Could not save window state: {error}");
        }
    });
    builder
        .setup(|_| {
            log::info!("Framewise started");
            Ok(())
        })
        .manage(AppState::default())
        .manage(move_files::MoveState::default())
        .invoke_handler(tauri::generate_handler![select_root, list_directory, move_videos_to_trash, move_folder_to_trash, duplicate::duplicate_video, rename::rename_video, rename::rename_folder, move_files::begin_move, move_files::move_session, move_files::list_move_directory, move_files::create_move_folder, move_files::create_media_folder, move_files::move_selected, preferences::load_preferences, preferences::save_preferences, inspect_video, prepare_preview, editor::prepare_edit, editor::video_frame_times, editor::export::export_edit])
        .run(tauri::generate_context!())
        .expect("error while building Tauri application");
}
