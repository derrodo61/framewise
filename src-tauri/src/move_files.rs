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
    AppState, DirectoryListing, list_folder, relocate_folder_previews, relocate_video_preview, rename::valid_stem,
    selected_root, thumbnail_name, video_file, within_root,
};

#[derive(Clone, Default)]
pub(crate) struct MoveState(Arc<Mutex<Option<MoveSession>>>);

struct MoveSession {
    sources: Vec<PathBuf>,
    source_folder: PathBuf,
    directories: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveSessionView {
    source_folder: String,
    names: Vec<String>,
    directories: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MoveResult {
    count: usize,
    source_folder: String,
    destination: String,
    directories: bool,
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
        directories: session.directories,
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
        return Err("Select at least one video or folder to move.".into());
    }
    let mut sources = Vec::with_capacity(paths.len());
    let mut seen = HashSet::new();
    let mut source_folder = None;
    let mut directories = None;
    let root = selected_root(&state)?;
    for path in paths {
        let kind = fs::symlink_metadata(&path)
            .map_err(|error| format!("Cannot open item: {error}"))?.file_type();
        if !(kind.is_dir() || kind.is_file()) {
            return Err("Select regular video files or folders only.".into());
        }
        let source = within_root(&path, &state)?;
        if source == root {
            return Err("The workspace root cannot be moved.".into());
        }
        if kind.is_file() && !video_file(&source) {
            return Err("Select supported video files only.".into());
        }
        if directories.is_some_and(|value| value != kind.is_dir()) {
            return Err("Select folders or videos together, rather than mixing both.".into());
        }
        directories = Some(kind.is_dir());
        let parent = source.parent().ok_or("Cannot find the item's folder")?;
        if source_folder
            .as_deref()
            .is_some_and(|folder| folder != parent)
        {
            return Err("Select items from the same folder.".into());
        }
        source_folder = Some(parent.to_path_buf());
        if seen.insert(source.clone()) {
            sources.push(source);
        }
    }
    let session = MoveSession {
        sources,
        source_folder: source_folder.ok_or("Select at least one item")?,
        directories: directories.unwrap_or(false),
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
        return Err("Select at least one item to move.".into());
    }
    if sources
        .iter()
        .any(|source| source.parent() == Some(destination))
    {
        return Err("Choose another folder for these items.".into());
    }
    let mut targets = Vec::with_capacity(sources.len());
    for source in sources {
        let kind = fs::symlink_metadata(source)
            .map_err(|error| format!("Cannot open {}: {error}", source.display()))?.file_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(format!("{} is no longer a regular file or folder.", source.display()));
        }
        if kind.is_dir() && destination.starts_with(source) {
            return Err("A folder cannot be moved into itself or one of its subfolders.".into());
        }
        let name = source.file_name().ok_or("Cannot read the item's name")?;
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
            if source.is_dir() { copy_directory_then_remove(source, destination) }
            else { copy_then_remove(source, destination) }
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

#[derive(PartialEq)]
struct TreeEntry {
    relative: PathBuf,
    directory: bool,
    size: u64,
    modified: Option<std::time::SystemTime>,
}

fn tree_snapshot(root: &Path) -> Result<Vec<TreeEntry>, String> {
    fn visit(root: &Path, path: &Path, entries: &mut Vec<TreeEntry>) -> Result<(), String> {
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if !(metadata.is_dir() || metadata.is_file()) {
            return Err(format!("Cannot move a folder between drives when it contains links or special files: {}", path.display()));
        }
        entries.push(TreeEntry {
            relative: path.strip_prefix(root).map_err(|error| error.to_string())?.to_path_buf(),
            directory: metadata.is_dir(),
            size: if metadata.is_file() { metadata.len() } else { 0 },
            modified: metadata.modified().ok(),
        });
        if metadata.is_dir() {
            let mut children = fs::read_dir(path).map_err(|error| error.to_string())?
                .map(|item| item.map(|item| item.path())).collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            children.sort();
            for child in children { visit(root, &child, entries)?; }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    Ok(entries)
}

fn copy_directory_then_remove(source: &Path, destination: &Path) -> Result<(), String> {
    // Copy the entire tree and verify it before removing anything from the source.
    let snapshot = tree_snapshot(source)?;
    fs::create_dir(destination).map_err(|error| format!("Cannot create destination folder: {error}"))?;
    let copied = (|| -> Result<(), String> {
        for entry in snapshot.iter().skip(1) {
            let old = source.join(&entry.relative);
            let new = destination.join(&entry.relative);
            if entry.directory {
                fs::create_dir(&new).map_err(|error| error.to_string())?;
            } else {
                let mut input = fs::File::open(&old).map_err(|error| error.to_string())?;
                let mut output = OpenOptions::new().write(true).create_new(true).open(&new)
                    .map_err(|error| error.to_string())?;
                let bytes = io::copy(&mut input, &mut output).map_err(|error| error.to_string())?;
                output.sync_all().map_err(|error| error.to_string())?;
                if bytes != entry.size { return Err("A source file changed while copying.".into()); }
                if let Some(modified) = entry.modified {
                    output.set_times(fs::FileTimes::new().set_modified(modified)).map_err(|error| error.to_string())?;
                }
            }
        }
        if tree_snapshot(source)? != snapshot {
            return Err("The source folder changed while copying. Nothing was removed from it.".into());
        }
        for entry in snapshot.iter().rev() {
            let old = source.join(&entry.relative);
            let new = destination.join(&entry.relative);
            let permissions = fs::symlink_metadata(&old).map_err(|error| error.to_string())?.permissions();
            fs::set_permissions(&new, permissions).map_err(|error| error.to_string())?;
        }
        Ok(())
    })();
    if let Err(error) = copied {
        return match fs::remove_dir_all(destination) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!("{error}. The original is intact; a partial copy remains at {}: {cleanup}", destination.display())),
        };
    }
    fs::remove_dir_all(source).map_err(|error| format!(
        "The folder was fully copied to {}, but the source could not be completely removed: {error}. The complete destination copy has been kept.", destination.display()
    ))
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
            let resolved = source.canonicalize().map_err(|error| format!("Cannot open selected item: {error}"))?;
            let kind = fs::symlink_metadata(source).map_err(|error| error.to_string())?.file_type();
            if resolved != *source || !resolved.starts_with(&root) || resolved == root {
                return Err("A selected item is outside the chosen folder or has changed.".into());
            }
            if (session.directories && !kind.is_dir()) || (!session.directories && (!kind.is_file() || !video_file(source))) {
                return Err("A selected item's type has changed.".into());
            }
        }
        let legacy_names: Vec<_> = session.sources.iter().map(|source| if session.directories { None } else { thumbnail_name(source) }).collect();
        move_paths(&session.sources, &destination)?;
        for (source, legacy_name) in session.sources.iter().zip(legacy_names) {
            if let Some(file_name) = source.file_name() {
                if session.directories { relocate_folder_previews(source, &destination.join(file_name)); }
                else { relocate_video_preview(source, &destination.join(file_name), legacy_name.as_deref()); }
            }
        }
        let result = MoveResult {
            count: session.sources.len(),
            source_folder: session.source_folder.to_string_lossy().into_owned(),
            destination: destination.to_string_lossy().into_owned(),
            directories: session.directories,
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
    use super::{copy_directory_then_remove, copy_then_remove, destination_listing, move_paths};
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

    #[test]
    fn folder_moves_reject_descendants_and_collisions_before_moving_anything() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("framewise-folder-move-{}-{stamp}", std::process::id()));
        let first = root.join("source/first");
        let second = root.join("source/second");
        let destination = root.join("destination");
        fs::create_dir_all(first.join("nested")).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::create_dir_all(destination.join("second")).unwrap();
        fs::write(first.join("nested/notes.txt"), b"keep non-video contents").unwrap();
        assert!(move_paths(std::slice::from_ref(&first), &first).is_err());
        assert!(move_paths(std::slice::from_ref(&first), &first.join("nested")).is_err());
        assert!(move_paths(&[first.clone(), second.clone()], &destination).is_err());
        assert!(first.exists() && second.exists());
        assert!(!destination.join("first").exists());
        fs::remove_dir(destination.join("second")).unwrap();
        move_paths(&[first.clone(), second.clone()], &destination).unwrap();
        assert!(!first.exists() && !second.exists());
        assert_eq!(fs::read(destination.join("first/nested/notes.txt")).unwrap(), b"keep non-video contents");
        assert!(destination.join("second").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cross_volume_folder_copy_preserves_nested_contents_empty_folders_and_file_dates() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("framewise-folder-copy-{}-{stamp}", std::process::id()));
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(source.join("nested/empty")).unwrap();
        let video = source.join("nested/clip.mp4");
        fs::write(&video, b"video bytes").unwrap();
        fs::write(source.join("other.txt"), b"other files move too").unwrap();
        let original_date = fs::metadata(&video).unwrap().modified().unwrap();
        copy_directory_then_remove(&source, &destination).unwrap();
        assert!(!source.exists());
        assert!(destination.join("nested/empty").is_dir());
        assert_eq!(fs::read(destination.join("nested/clip.mp4")).unwrap(), b"video bytes");
        assert_eq!(fs::read(destination.join("other.txt")).unwrap(), b"other files move too");
        assert_eq!(fs::metadata(destination.join("nested/clip.mp4")).unwrap().modified().unwrap(), original_date);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn cross_volume_folder_copy_rejects_links_without_removing_source() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("framewise-folder-link-{}-{stamp}", std::process::id()));
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(&source).unwrap();
        fs::write(root.join("outside.txt"), b"outside").unwrap();
        std::os::unix::fs::symlink(root.join("outside.txt"), source.join("link")).unwrap();
        assert!(copy_directory_then_remove(&source, &destination).is_err());
        assert!(source.exists() && !destination.exists());
        assert_eq!(fs::read(root.join("outside.txt")).unwrap(), b"outside");
        fs::remove_dir_all(root).unwrap();
    }
}
