use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{AppState, DirectoryListing, relocate_folder_previews, relocate_video_preview, selected_root, thumbnail_name, media_file, within_root};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenameResult {
    listing: DirectoryListing,
    renamed_path: String,
}

pub(crate) fn valid_stem(stem: &str) -> bool {
    if stem.is_empty()
        || stem.trim() != stem
        || stem.ends_with('.')
        || stem
            .chars()
            .any(|character| character.is_control() || "<>:\"/\\|?*".contains(character))
    {
        return false;
    }
    let reserved = stem.split('.').next().unwrap_or("").to_ascii_uppercase();
    !matches!(
        reserved.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn rename_file(source: &Path, new_stem: &str) -> Result<PathBuf, String> {
    if !valid_stem(new_stem) {
        return Err(
            "Choose a valid file name without path separators or reserved characters.".into(),
        );
    }
    let extension = source
        .extension()
        .and_then(|part| part.to_str())
        .ok_or("Cannot read the video extension")?;
    let current_stem = source
        .file_stem()
        .and_then(|part| part.to_str())
        .ok_or("Cannot read the video filename")?;
    if new_stem == current_stem {
        return Err("The video already has that name.".into());
    }
    let destination = source.with_file_name(format!("{new_stem}.{extension}"));
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err("A file with that name already exists.".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot check the new filename: {error}")),
    }
    fs::rename(source, &destination).map_err(|error| format!("Could not rename video: {error}"))?;
    Ok(destination)
}

fn rename_directory(source: &Path, new_name: &str) -> Result<PathBuf, String> {
    if !valid_stem(new_name) {
        return Err(
            "Choose a valid folder name without path separators or reserved characters.".into(),
        );
    }
    if source
        .file_name()
        .is_some_and(|current| current == new_name)
    {
        return Err("The folder already has that name.".into());
    }
    let destination = source.with_file_name(new_name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => return Err("A file or folder with that name already exists.".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot check the new folder name: {error}")),
    }
    fs::rename(source, &destination)
        .map_err(|error| format!("Could not rename folder: {error}"))?;
    Ok(destination)
}

#[tauri::command]
pub(crate) async fn rename_video(
    path: String,
    new_stem: String,
    state: tauri::State<'_, AppState>,
) -> Result<RenameResult, String> {
    let file_type = fs::symlink_metadata(&path)
        .map_err(|error| format!("Cannot open video: {error}"))?
        .file_type();
    if !file_type.is_file() {
        return Err("Select a regular media file".into());
    }
    let root = selected_root(&state)?;
    let source = within_root(&path, &state)?;
    if !media_file(&source) {
        return Err("Select a supported media file".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        if !valid_stem(&new_stem) { return Err("Choose a valid file name.".into()); }
        let target = source.with_file_name(format!("{new_stem}.{}", source.extension().ok_or("Cannot read extension")?.to_string_lossy()));
        let operation = crate::catalog_operations::Operation::begin(crate::catalog_operations::Action::Move, vec![source.clone()], vec![Some(target)])?;
        let legacy_name = thumbnail_name(&source);
        let renamed = match rename_file(&source, &new_stem) { Ok(path) => path, Err(error) => { operation.cancel_if_unchanged(); return Err(error); } };
        relocate_video_preview(&source, &renamed, legacy_name.as_deref());
        operation.finish()?;
        let folder = source.parent().ok_or("Cannot find the video's folder")?;
        Ok(RenameResult {
            listing: crate::catalogued_folder(folder, &root)?,
            renamed_path: renamed.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|error| format!("Rename task failed: {error}"))?
}

#[tauri::command]
pub(crate) async fn rename_folder(
    path: String,
    new_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<RenameResult, String> {
    let file_type = fs::symlink_metadata(&path)
        .map_err(|error| format!("Cannot open folder: {error}"))?
        .file_type();
    if !file_type.is_dir() {
        return Err("Select a regular folder".into());
    }
    let root = selected_root(&state)?;
    let source = within_root(&path, &state)?;
    if source == root {
        return Err("The selected media folder cannot be renamed here.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        if !valid_stem(&new_name) { return Err("Choose a valid folder name.".into()); }
        let operation = crate::catalog_operations::Operation::begin(crate::catalog_operations::Action::Move, vec![source.clone()], vec![Some(source.with_file_name(&new_name))])?;
        let renamed = match rename_directory(&source, &new_name) { Ok(path) => path, Err(error) => { operation.cancel_if_unchanged(); return Err(error); } };
        relocate_folder_previews(&source, &renamed);
        operation.finish()?;
        let parent = source.parent().ok_or("Cannot find the folder's parent")?;
        Ok(RenameResult {
            listing: crate::catalogued_folder(parent, &root)?,
            renamed_path: renamed.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|error| format!("Rename task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::{rename_directory, rename_file, valid_stem};
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn rename_keeps_extension_and_does_not_overwrite() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder =
            std::env::temp_dir().join(format!("framewise-rename-{}-{stamp}", std::process::id()));
        fs::create_dir(&folder).unwrap();
        let source = folder.join("clip.mp4");
        let occupied = folder.join("taken.mp4");
        fs::write(&source, b"video").unwrap();
        fs::write(&occupied, b"other").unwrap();
        assert!(rename_file(&source, "taken").is_err());
        assert_eq!(fs::read(&occupied).unwrap(), b"other");
        let renamed = rename_file(&source, "new clip").unwrap();
        assert_eq!(renamed.file_name().unwrap(), "new clip.mp4");
        assert_eq!(fs::read(&renamed).unwrap(), b"video");
        assert!(!source.exists());
        fs::remove_file(renamed).unwrap();
        fs::remove_file(occupied).unwrap();
        fs::remove_dir(folder).unwrap();
    }

    #[test]
    fn rejects_names_that_are_invalid_on_supported_platforms() {
        for stem in [
            "",
            " ",
            " clip",
            "clip ",
            "clip.",
            "../clip",
            "a\\b",
            "CON",
            "NUL.foo",
            "bad:name",
            "bad\nname",
        ] {
            assert!(!valid_stem(stem), "accepted {stem:?}");
        }
        assert!(valid_stem("My clip (2)"));
    }

    #[test]
    fn folder_rename_keeps_contents_and_rejects_existing_names() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let parent = std::env::temp_dir().join(format!(
            "framewise-folder-rename-{}-{stamp}",
            std::process::id()
        ));
        let original = parent.join("original");
        let occupied = parent.join("occupied");
        fs::create_dir_all(&original).unwrap();
        fs::create_dir(&occupied).unwrap();
        fs::write(original.join("clip.mp4"), b"video").unwrap();
        assert!(rename_directory(&original, "occupied").is_err());
        assert!(rename_directory(&original, "../outside").is_err());
        let renamed = rename_directory(&original, "new folder").unwrap();
        assert_eq!(fs::read(renamed.join("clip.mp4")).unwrap(), b"video");
        assert!(!original.exists());
        fs::remove_dir_all(parent).unwrap();
    }
}
