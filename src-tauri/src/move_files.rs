use serde::Serialize;
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::Emitter;

use crate::{
    AppState, DirectoryListing, list_folder, relocate_video_preview, rename::valid_stem,
    selected_root, thumbnail_name, video_file, within_root,
};

#[derive(Clone, Default)]
pub(crate) struct MoveState(Arc<Mutex<Option<MoveSession>>>);

struct MoveSession {
    sources: Vec<PathBuf>,
    source_folder: PathBuf,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveSessionView {
    source_folder: String,
    names: Vec<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveResult {
    count: usize,
    source_folder: String,
    destination: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreatedFolder {
    listing: DirectoryListing,
    created_path: String,
}

fn session_view(session: &MoveSession) -> MoveSessionView {
    MoveSessionView {
        source_folder: session.source_folder.to_string_lossy().into_owned(),
        names: session
            .sources
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect(),
    }
}

fn destination_listing(path: &Path) -> Result<DirectoryListing, String> {
    let directory = path
        .canonicalize()
        .map_err(|error| format!("Cannot open destination folder: {error}"))?;
    if !directory.is_dir() {
        return Err("Choose an existing destination folder.".into());
    }
    let filesystem_root = directory
        .ancestors()
        .last()
        .ok_or("Cannot find the destination root")?;
    list_folder(&directory, filesystem_root)
}

#[tauri::command]
pub(crate) fn list_move_directory(path: String) -> Result<DirectoryListing, String> {
    destination_listing(Path::new(&path))
}

#[tauri::command]
pub(crate) fn begin_move(
    paths: Vec<String>,
    state: tauri::State<'_, AppState>,
    move_state: tauri::State<'_, MoveState>,
) -> Result<MoveSessionView, String> {
    if paths.is_empty() {
        return Err("Select at least one video to move.".into());
    }
    let mut sources = Vec::with_capacity(paths.len());
    let mut seen = HashSet::new();
    let mut source_folder = None;
    for path in paths {
        if !fs::symlink_metadata(&path)
            .map_err(|error| format!("Cannot open video: {error}"))?
            .file_type()
            .is_file()
        {
            return Err("Select regular video files only.".into());
        }
        let source = within_root(&path, &state)?;
        if !video_file(&source) {
            return Err("Select supported video files only.".into());
        }
        let parent = source.parent().ok_or("Cannot find a video's folder")?;
        if source_folder
            .as_deref()
            .is_some_and(|folder| folder != parent)
        {
            return Err("Select videos from the same folder.".into());
        }
        source_folder = Some(parent.to_path_buf());
        if seen.insert(source.clone()) {
            sources.push(source);
        }
    }
    let session = MoveSession {
        sources,
        source_folder: source_folder.ok_or("Select at least one video")?,
    };
    let view = session_view(&session);
    *move_state
        .0
        .lock()
        .map_err(|_| "Move state is unavailable")? = Some(session);
    Ok(view)
}

#[tauri::command]
pub(crate) fn move_session(
    move_state: tauri::State<'_, MoveState>,
) -> Result<MoveSessionView, String> {
    let guard = move_state
        .0
        .lock()
        .map_err(|_| "Move state is unavailable")?;
    guard
        .as_ref()
        .map(session_view)
        .ok_or("No files are selected for moving".into())
}

#[tauri::command]
pub(crate) fn create_move_folder(parent: String, name: String) -> Result<DirectoryListing, String> {
    let parent = Path::new(&parent)
        .canonicalize()
        .map_err(|error| format!("Cannot open parent folder: {error}"))?;
    let folder = create_folder(&parent, &name)?;
    destination_listing(&folder)
}

fn create_folder(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_stem(name) {
        return Err(
            "Choose a valid folder name without path separators or reserved characters.".into(),
        );
    }
    if !parent.is_dir() {
        return Err("Choose an existing folder.".into());
    }
    let folder = parent.join(name);
    fs::create_dir(&folder).map_err(|error| format!("Could not create folder: {error}"))?;
    Ok(folder)
}

#[tauri::command]
pub(crate) fn create_media_folder(
    parent: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<CreatedFolder, String> {
    let root = selected_root(&state)?;
    let parent = within_root(&parent, &state)?;
    let folder = create_folder(&parent, &name)?;
    Ok(CreatedFolder {
        listing: list_folder(&parent, &root)?,
        created_path: folder.to_string_lossy().into_owned(),
    })
}

fn move_paths(sources: &[PathBuf], destination: &Path) -> Result<(), String> {
    if sources.is_empty() {
        return Err("Select at least one video to move.".into());
    }
    if sources
        .iter()
        .any(|source| source.parent() == Some(destination))
    {
        return Err("Choose another folder for these videos.".into());
    }
    let mut targets = Vec::with_capacity(sources.len());
    for source in sources {
        if !fs::symlink_metadata(source)
            .map_err(|error| format!("Cannot open {}: {error}", source.display()))?
            .file_type()
            .is_file()
        {
            return Err(format!("{} is no longer a regular file.", source.display()));
        }
        let name = source.file_name().ok_or("Cannot read a video filename")?;
        let target = destination.join(name);
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(format!(
                    "{} already exists in the destination.",
                    name.to_string_lossy()
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Cannot check destination: {error}")),
        }
        targets.push(target);
    }
    let mut completed: Vec<(&PathBuf, &PathBuf)> = Vec::new();
    for (source, target) in sources.iter().zip(&targets) {
        if let Err(error) = move_one(source, target) {
            let mut rollback_failures = Vec::new();
            for (old, new) in completed.into_iter().rev() {
                if let Err(rollback) = move_one(new, old) {
                    rollback_failures.push(format!("{}: {rollback}", new.display()));
                }
            }
            let detail = if rollback_failures.is_empty() {
                "Earlier moves were restored.".to_string()
            } else {
                format!("Could not restore: {}", rollback_failures.join("; "))
            };
            return Err(format!(
                "Could not move {}: {error}. {detail}",
                source.display()
            ));
        }
        completed.push((source, target));
    }
    Ok(())
}

fn move_one(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
            copy_then_remove(source, destination)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn copy_then_remove(source: &Path, destination: &Path) -> Result<(), String> {
    let before = fs::metadata(source).map_err(|error| format!("Cannot read source: {error}"))?;
    let mut input =
        fs::File::open(source).map_err(|error| format!("Cannot open source: {error}"))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("Cannot create destination: {error}"))?;
    let copied = io::copy(&mut input, &mut output).and_then(|bytes| {
        output.flush()?;
        output.sync_all()?;
        Ok(bytes)
    });
    drop(output);
    drop(input);
    let unchanged = fs::metadata(source).is_ok_and(|after| {
        after.len() == before.len() && after.modified().ok() == before.modified().ok()
    });
    if !matches!(copied, Ok(bytes) if bytes == before.len()) || !unchanged {
        fs::remove_file(destination).map_err(|error| {
            format!(
                "Copy failed and the partial file at {} could not be removed: {error}",
                destination.display()
            )
        })?;
        return Err("The source changed or could not be copied completely.".into());
    }
    if let Err(error) = fs::set_permissions(destination, before.permissions()) {
        log::warn!("Could not copy video permissions: {error}");
    }
    if let Err(error) = fs::remove_file(source) {
        fs::remove_file(destination).map_err(|cleanup| {
            format!(
                "Could not remove source ({error}) or copied destination ({}): {cleanup}",
                destination.display()
            )
        })?;
        return Err(format!("Could not remove source after copying: {error}"));
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn move_selected(
    destination: String,
    state: tauri::State<'_, AppState>,
    move_state: tauri::State<'_, MoveState>,
    app: tauri::AppHandle,
) -> Result<MoveResult, String> {
    let root = selected_root(&state)?;
    let destination = Path::new(&destination)
        .canonicalize()
        .map_err(|error| format!("Cannot open destination folder: {error}"))?;
    if !destination.is_dir() {
        return Err("Choose an existing destination folder.".into());
    }
    let move_state = move_state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = move_state
            .0
            .lock()
            .map_err(|_| "Move state is unavailable")?;
        let session = guard.as_ref().ok_or("No files are selected for moving")?;
        for source in &session.sources {
            if !source.starts_with(&root) || !video_file(source) {
                return Err("A selected file is outside the chosen folder.".into());
            }
        }
        let legacy_names: Vec<_> = session.sources.iter().map(|source| thumbnail_name(source)).collect();
        move_paths(&session.sources, &destination)?;
        for (source, legacy_name) in session.sources.iter().zip(legacy_names) {
            if let Some(file_name) = source.file_name() {
                relocate_video_preview(source, &destination.join(file_name), legacy_name.as_deref());
            }
        }
        let result = MoveResult {
            count: session.sources.len(),
            source_folder: session.source_folder.to_string_lossy().into_owned(),
            destination: destination.to_string_lossy().into_owned(),
        };
        *guard = None;
        if let Err(error) = app.emit_to("main", "files-moved", &result) {
            log::warn!("Could not notify main window after moving files: {error}");
        }
        Ok(result)
    })
    .await
    .map_err(|error| format!("Move task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::{copy_then_remove, destination_listing, move_paths};
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn moves_multiple_files_and_refuses_collisions_without_partial_move() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("framewise-move-{}-{stamp}", std::process::id()));
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        assert_eq!(
            destination_listing(&source).unwrap().parent.as_deref(),
            root.canonicalize().unwrap().to_str()
        );
        let first = source.join("first.mp4");
        let second = source.join("second.mp4");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        fs::write(destination.join("second.mp4"), b"existing").unwrap();
        assert!(move_paths(&[first.clone(), second.clone()], &destination).is_err());
        assert!(first.exists() && second.exists());
        assert_eq!(
            fs::read(destination.join("second.mp4")).unwrap(),
            b"existing"
        );
        fs::remove_file(destination.join("second.mp4")).unwrap();
        move_paths(&[first.clone(), second.clone()], &destination).unwrap();
        assert!(!first.exists() && !second.exists());
        assert_eq!(fs::read(destination.join("first.mp4")).unwrap(), b"first");
        assert_eq!(fs::read(destination.join("second.mp4")).unwrap(), b"second");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cross_volume_fallback_copies_then_removes_source() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "framewise-cross-volume-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let source = root.join("source.mp4");
        let destination = root.join("destination.mp4");
        fs::write(&source, b"complete video bytes").unwrap();
        copy_then_remove(&source, &destination).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"complete video bytes");
        fs::remove_dir_all(root).unwrap();
    }
}
