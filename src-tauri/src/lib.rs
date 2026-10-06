use serde::Serialize;
use std::{collections::{hash_map::DefaultHasher, HashSet}, fs, hash::{Hash, Hasher}, io, path::{Path, PathBuf}, process::{Command, Output}, sync::Mutex, time::{Instant, UNIX_EPOCH}};
use tauri::Manager;
mod editor;
mod duplicate;
mod rename;
mod move_files;
mod preferences;
mod preview_index;
mod database;
mod catalog;
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
    modified_at: Option<u64>,
    video_id: Option<i64>,
    #[serde(skip)]
    modified_ns: Option<String>,
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
    list_folder(&root, &root).map(register_browsed_videos)
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
        let metadata = item.metadata().ok();
        entries.push(FileEntry {
            name: item.file_name().to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
            is_directory: file_type.is_dir(),
            video_id: None,
            modified_ns: metadata.as_ref().and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|duration| duration.as_nanos().to_string()),
            size: if file_type.is_file() { metadata.as_ref().map(|metadata| metadata.len()) } else { None },
            modified_at: metadata.and_then(|metadata| metadata.modified().ok())
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|duration| u64::try_from(duration.as_millis()).ok()),
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
    list_folder(&resolved, &root).map(register_browsed_videos)
}

fn register_browsed_videos(mut listing: DirectoryListing) -> DirectoryListing {
    let present: Vec<_> = listing.entries.iter().filter(|entry| !entry.is_directory).map(|entry| entry.path.clone()).collect();
    let observed: Vec<_> = listing.entries.iter().filter(|entry| !entry.is_directory).filter_map(|entry| Some(catalog::ObservedVideo {
        path: entry.path.clone(), name: entry.name.clone(), size: entry.size?, modified_ns: entry.modified_ns.clone()?,
    })).collect();
    match catalog::register_folder(Path::new(&listing.path), &present, &observed) {
        Ok(ids) => { for entry in &mut listing.entries { entry.video_id = ids.get(&entry.path).copied(); } }
        Err(error) => log::error!("Could not register browsed videos: {error}"),
    }
    listing
}

#[tauri::command]
async fn reveal_in_file_manager(path: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let resolved = within_root(&path, &state)?;
    tauri::async_runtime::spawn_blocking(move || {
        tauri_plugin_opener::reveal_item_in_dir(&resolved)
            .map_err(|error| format!("Could not show item in the file manager: {error}"))
    }).await.map_err(|error| format!("File manager task failed: {error}"))?
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
        let preview_names: Vec<_> = files.iter().map(|file| thumbnail_name(file)).collect();
        let error = trash::delete_all(&files).err().map(|cause| {
            log::error!("Could not move every video to Trash: {cause}");
            format!("Could not move every video to Trash: {cause}")
        });
        let mut moved_count = 0;
        for (file, preview_name) in files.iter().zip(preview_names) {
            if matches!(fs::symlink_metadata(file), Err(cause) if cause.kind() == io::ErrorKind::NotFound) {
                moved_count += 1;
                forget_video_preview(file, preview_name.as_deref());
            }
        }
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
    let preview_names = video_preview_names_in_folder(&resolved);
    let parent = resolved.parent().ok_or("Cannot find the folder's parent")?;
    trash::delete(&resolved).map_err(|error| {
        log::error!("Could not move folder to Trash: {error}");
        format!("Could not move folder to Trash: {error}")
    })?;
    forget_folder_previews(&resolved, &preview_names);
    list_folder(parent, &root)
}

#[tauri::command]
async fn inspect_video(path: String, state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    let resolved = within_root(&path, &state)?;
    if !resolved.is_file() || !video_file(&resolved) {
        return Err("Select a supported video file".into());
    }
    tauri::async_runtime::spawn_blocking(move || inspect_video_file(&resolved))
        .await.map_err(|error| format!("Metadata task failed: {error}"))?
}

fn inspect_video_file(resolved: &Path) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    let output = run_ffprobe(resolved)
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
    let result = serde_json::from_slice(&output.stdout).map_err(|error| {
        log::error!("ffprobe returned invalid JSON: {error}");
        format!("Invalid ffprobe output: {error}")
    });
    if started.elapsed().as_millis() > 250 {
        log::info!("Slow metadata read: {} ms, video={}", started.elapsed().as_millis(), resolved.display());
    }
    result
}

pub(crate) fn media_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let command = Command::new(program);
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        let mut command = command;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW for the installed GUI app.
        command
    };
    command
}

fn execute_ffprobe(probe: &Path, video: &Path, args: &[&str]) -> io::Result<Output> {
    media_command(probe)
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
    media_command(program)
        .args(["-hide_banner", "-loglevel", "error", "-ss", seek, "-i"])
        .arg(video)
        .args(["-frames:v", "1", "-vf", "scale=480:-2", "-y"])
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

pub(crate) fn thumbnail_name(video: &Path) -> Option<String> {
    let metadata = fs::metadata(video).ok()?;
    let mut hash = DefaultHasher::new();
    video.hash(&mut hash);
    metadata.len().hash(&mut hash);
    metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos().hash(&mut hash);
    Some(format!("{:016x}", hash.finish()))
}

fn thumbnail_version(video: &Path) -> Option<String> {
    let metadata = fs::metadata(video).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    Some(format!("{}:{modified}", metadata.len()))
}

fn video_preview_names_in_folder(folder: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(folder) = folders.pop() {
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => { log::warn!("Could not scan folder for cached previews: {error}"); continue; }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => { log::warn!("Could not scan a folder entry for cached previews: {error}"); continue; }
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => { log::warn!("Could not identify a file while scanning cached previews: {error}"); continue; }
            };
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                folders.push(path);
            } else if file_type.is_file() && video_file(&path)
                && let Some(name) = thumbnail_name(&path)
            {
                names.push(name);
            }
        }
    }
    names
}

fn remove_preview_images(name: &str) {
    let directory = match preview_cache_dir() {
        Ok(directory) => directory,
        Err(error) => { log::warn!("Could not locate preview cache for cleanup: {error}"); return; }
    };
    for extension in ["png", "jpg"] {
        let image = directory.join(format!("{name}.{extension}"));
        if let Err(error) = fs::remove_file(&image)
            && error.kind() != io::ErrorKind::NotFound
        {
            log::warn!("Could not remove cached preview {}: {error}", image.display());
        }
    }
}

fn remove_indexed_image(image_name: &str) {
    let image = Path::new(image_name);
    if !preview_filename(image) || image.file_name().is_none_or(|name| name != image_name) {
        log::warn!("Ignoring invalid preview filename in index: {image_name}");
        return;
    }
    let Ok(directory) = preview_cache_dir() else { return; };
    let path = directory.join(image_name);
    if let Err(error) = fs::remove_file(&path)
        && error.kind() != io::ErrorKind::NotFound
    {
        log::warn!("Could not remove cached preview {}: {error}", path.display());
    }
}

pub(crate) fn forget_video_preview(video: &Path, legacy_name: Option<&str>) {
    match preview_index::forget(video) {
        Ok(Some(image)) => remove_indexed_image(&image),
        Ok(None) => {}
        Err(error) => log::warn!("Could not update preview index after deleting video: {error}"),
    }
    if let Some(name) = legacy_name {
        remove_preview_images(name);
    }
}

fn forget_folder_previews(folder: &Path, legacy_names: &[String]) {
    match preview_index::forget_folder(folder) {
        Ok(images) => { for image in images { remove_indexed_image(&image); } }
        Err(error) => log::warn!("Could not update preview index after deleting folder: {error}"),
    }
    for name in legacy_names {
        remove_preview_images(name);
    }
}

pub(crate) fn relocate_video_preview(old: &Path, new: &Path, legacy_name: Option<&str>) {
    match preview_index::relocate(old, new) {
        Ok(Some(displaced)) => remove_indexed_image(&displaced),
        Ok(None) => {}
        Err(error) => { log::warn!("Could not update preview index after moving video: {error}"); return; }
    }
    if let (Some(name), Some(version)) = (legacy_name, thumbnail_version(new)) {
        let indexed = preview_index::find(new).ok().flatten();
        if indexed.is_none() {
            let Ok(directory) = preview_cache_dir() else { return; };
            for extension in ["png", "jpg"] {
                let image_name = format!("{name}.{extension}");
                if valid_thumbnail(&directory.join(&image_name)) {
                    if let Err(error) = preview_index::record(new, &version, &image_name) {
                        log::warn!("Could not index moved video preview: {error}");
                    }
                    break;
                }
            }
        }
    }
}

pub(crate) fn relocate_folder_previews(old: &Path, new: &Path) {
    match preview_index::relocate_folder(old, new) {
        Ok(displaced) => { for image in displaced { remove_indexed_image(&image); } }
        Err(error) => log::warn!("Could not update preview index after moving folder: {error}"),
    }
}

fn preview_cache_dir() -> Result<PathBuf, String> {
    Ok(preferences::settings_dir()?.join("previews"))
}

fn ensure_preview_cache_dir() -> Result<PathBuf, String> {
    let directory = preview_cache_dir()?;
    fs::create_dir_all(&directory).map_err(|error| format!("Cannot create preview cache: {error}"))?;
    Ok(directory)
}

fn valid_thumbnail(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|file| file.file_type().is_file() && file.len() > 0)
}

fn preview_filename(path: &Path) -> bool {
    matches!(path.extension().and_then(|extension| extension.to_str()), Some("png" | "jpg"))
        && path.file_stem().and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.len() == 16 && stem.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn move_cached_thumbnail(old: &Path, target: &Path) -> io::Result<()> {
    if fs::rename(old, target).is_ok() {
        return Ok(());
    }
    let expected_size = fs::metadata(old)?.len();
    match fs::copy(old, target) {
        Ok(copied_size) if copied_size == expected_size => fs::remove_file(old),
        result => {
            let _ = fs::remove_file(target);
            match result {
                Ok(_) => Err(io::Error::other("Preview copy was incomplete")),
                Err(error) => Err(error),
            }
        }
    }
}

fn migrate_preview_cache(app: &tauri::AppHandle) -> Result<(), String> {
    let old_directory = app.path().app_cache_dir()
        .map_err(|error| format!("Cannot locate previous preview cache: {error}"))?.join("previews");
    if !old_directory.is_dir() {
        return Ok(());
    }
    let directory = ensure_preview_cache_dir()?;
    let entries = fs::read_dir(&old_directory)
        .map_err(|error| format!("Cannot read previous preview cache: {error}"))?;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => { log::warn!("Could not read a previous preview: {error}"); continue; }
        };
        let old = entry.path();
        if !preview_filename(&old) || !valid_thumbnail(&old) {
            continue;
        }
        let target = directory.join(entry.file_name());
        if valid_thumbnail(&target) {
            if let Err(error) = fs::remove_file(&old) {
                log::warn!("Could not remove duplicate preview: {error}");
            }
            continue;
        }
        if target.exists() && fs::remove_file(&target).is_err() {
            log::warn!("Could not replace an incomplete preview: {}", target.display());
            continue;
        }
        if let Err(error) = move_cached_thumbnail(&old, &target) {
            log::warn!("Could not move previous preview: {error}");
        }
    }
    Ok(())
}

fn existing_thumbnail(video: &Path, app: &tauri::AppHandle) -> Option<PathBuf> {
    let version = thumbnail_version(video)?;
    match preview_index::find(video) {
        Ok(Some(entry)) => {
            let image = Path::new(&entry.image_name);
            if entry.version == version && preview_filename(image)
                && image.file_name().and_then(|name| name.to_str()) == Some(entry.image_name.as_str())
            {
                let target = preview_cache_dir().ok()?.join(&entry.image_name);
                if valid_thumbnail(&target) {
                    return Some(target);
                }
            }
            match preview_index::forget(video) {
                Ok(Some(old)) => remove_indexed_image(&old),
                Ok(None) => {}
                Err(error) => log::warn!("Could not clear outdated preview index entry: {error}"),
            }
        }
        Ok(None) => {}
        Err(error) => log::warn!("Could not read preview index: {error}"),
    }
    let name = thumbnail_name(video)?;
    let directory = preview_cache_dir().ok()?;
    for extension in ["png", "jpg"] {
        let target = directory.join(format!("{name}.{extension}"));
        if valid_thumbnail(&target) {
            register_thumbnail(video, &version, &target);
            return Some(target);
        }
    }
    let old_directory = app.path().app_cache_dir().ok()?.join("previews");
    for extension in ["png", "jpg"] {
        let old = old_directory.join(format!("{name}.{extension}"));
        if !valid_thumbnail(&old) {
            continue;
        }
        let target = directory.join(format!("{name}.{extension}"));
        if let Err(error) = fs::create_dir_all(&directory).and_then(|_| move_cached_thumbnail(&old, &target)) {
            log::warn!("Could not move cached preview into .framewise: {error}");
            return Some(old);
        }
        register_thumbnail(video, &version, &target);
        return Some(target);
    }
    None
}

fn register_thumbnail(video: &Path, version: &str, thumbnail: &Path) {
    let Ok(directory) = preview_cache_dir() else { return; };
    if thumbnail.parent() != Some(directory.as_path()) {
        return;
    }
    let Some(image_name) = thumbnail.file_name().and_then(|name| name.to_str()) else { return; };
    match preview_index::record(video, version, image_name) {
        Ok(Some(old)) => remove_indexed_image(&old),
        Ok(None) => {}
        Err(error) => log::warn!("Could not index video preview: {error}"),
    }
}

#[tauri::command]
fn preview_cache_directory() -> Result<String, String> {
    Ok(ensure_preview_cache_dir()?.to_string_lossy().into_owned())
}

#[tauri::command]
fn preview_index_location() -> Result<String, String> {
    Ok(preview_index::database_path()?.to_string_lossy().into_owned())
}

fn create_thumbnail(video: &Path, app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Some(thumbnail) = existing_thumbnail(video, app) {
        return Some(thumbnail);
    }
    let version = thumbnail_version(video)?;
    let thumbnail = preview_cache_dir().ok()?.join(format!("{}.png", thumbnail_name(video)?));
    if let Err(error) = fs::create_dir_all(thumbnail.parent()?) {
        log::warn!("Could not create preview cache: {error}");
        return None;
    }
    for seek in ["1", "0"] {
        match run_ffmpeg(video, &thumbnail, seek) {
            Ok(output) if output.status.success() && thumbnail.metadata().is_ok_and(|file| file.len() > 0) => {
                if thumbnail_version(video).as_deref() != Some(version.as_str()) {
                    let _ = fs::remove_file(&thumbnail);
                    return None;
                }
                register_thumbnail(video, &version, &thumbnail);
                return Some(thumbnail);
            }
            Ok(output) => log::warn!("Could not create video thumbnail: {}", String::from_utf8_lossy(&output.stderr).trim()),
            Err(error) => { log::warn!("Could not start ffmpeg for thumbnail: {error}"); break; }
        }
        let _ = fs::remove_file(&thumbnail);
    }
    None
}

#[tauri::command]
fn prepare_preview(path: String, state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Result<Preview, String> {
    let started = Instant::now();
    let video = within_root(&path, &state)?;
    if !video.is_file() || !video_file(&video) {
        return Err("Select a supported video file".into());
    }
    let video_version = editor::signature(&video)?;
    app.asset_protocol_scope().allow_file(&video).map_err(|error| format!("Cannot open video preview: {error}"))?;
    let lookup_started = Instant::now();
    let thumbnail_path = existing_thumbnail(&video, &app).and_then(|thumbnail| {
        if let Err(error) = app.asset_protocol_scope().allow_file(&thumbnail) {
            log::warn!("Could not expose video thumbnail: {error}");
            None
        } else { Some(thumbnail.to_string_lossy().into_owned()) }
    });
    let elapsed = started.elapsed();
    if elapsed.as_millis() > 250 {
        log::info!("Slow preview lookup: total={} ms, cache={} ms, cached={}, video={}", elapsed.as_millis(), lookup_started.elapsed().as_millis(), thumbnail_path.is_some(), video.display());
    }
    Ok(Preview { video_path: video.to_string_lossy().into_owned(), video_version, thumbnail_path })
}

#[tauri::command]
async fn generate_preview_thumbnail(path: String, state: tauri::State<'_, AppState>, app: tauri::AppHandle) -> Result<Option<String>, String> {
    let video = within_root(&path, &state)?;
    if !video.is_file() || !video_file(&video) {
        return Err("Select a supported video file".into());
    }
    let worker_app = app.clone();
    let thumbnail = tauri::async_runtime::spawn_blocking(move || create_thumbnail(&video, &worker_app))
        .await.map_err(|error| format!("Could not prepare video thumbnail: {error}"))?;
    Ok(thumbnail.and_then(|thumbnail| {
        if let Err(error) = app.asset_protocol_scope().allow_file(&thumbnail) {
            log::warn!("Could not expose video thumbnail: {error}");
            None
        } else { Some(thumbnail.to_string_lossy().into_owned()) }
    }))
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
        .setup(|app| {
            log::info!("Framewise started");
            if let Err(error) = ensure_preview_cache_dir() {
                log::error!("Could not prepare preview cache: {error}");
            } else if let Err(error) = migrate_preview_cache(app.handle()) {
                log::warn!("Could not migrate previous preview cache: {error}");
            }
            if let Err(error) = database::initialize() {
                log::error!("Could not prepare Framewise database: {error}");
            }
            Ok(())
        })
        .manage(AppState::default())
        .manage(move_files::MoveState::default())
        .invoke_handler(tauri::generate_handler![select_root, list_directory, reveal_in_file_manager, move_videos_to_trash, move_folder_to_trash, duplicate::duplicate_video, rename::rename_video, rename::rename_folder, move_files::begin_move, move_files::move_session, move_files::list_move_directory, move_files::create_move_folder, move_files::create_media_folder, move_files::move_selected, preferences::load_preferences, preferences::save_preferences, inspect_video, prepare_preview, generate_preview_thumbnail, preview_cache_directory, preview_index_location, editor::prepare_edit, editor::video_frame_times, editor::export::export_edit])
        .run(tauri::generate_context!())
        .expect("error while building Tauri application");
}
