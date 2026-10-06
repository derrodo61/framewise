use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use crate::{AppState, DirectoryListing, selected_root, video_file, within_root};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DuplicateResult {
    listing: DirectoryListing,
    duplicated_path: String,
}

fn duplicate_file_with(source: &Path, mut before_copy: impl FnMut(&Path) -> Result<(), String>) -> Result<PathBuf, String> {
    let stem = source
        .file_stem()
        .and_then(|part| part.to_str())
        .ok_or("Cannot read the video filename")?;
    let extension = source
        .extension()
        .and_then(|part| part.to_str())
        .ok_or("Cannot read the video extension")?;
    let (base, first_number) = stem
        .rsplit_once(" (")
        .and_then(|(base, suffix)| {
            suffix
                .strip_suffix(')')
                .and_then(|number| number.parse::<u32>().ok())
                .filter(|number| *number > 0 && *number < u32::MAX)
                .map(|number| (base, number + 1))
        })
        .unwrap_or((stem, 1));
    let before = fs::metadata(source).map_err(|error| format!("Cannot read video: {error}"))?;
    let mut input =
        fs::File::open(source).map_err(|error| format!("Cannot open video: {error}"))?;

    for offset in 0..10_000_u32 {
        let Some(number) = first_number.checked_add(offset) else {
            break;
        };
        let destination = source.with_file_name(format!("{base} ({number}).{extension}"));
        let mut output = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create duplicate: {error}")),
        };
        if let Err(error) = before_copy(&destination) {
            drop(output);
            fs::remove_file(&destination).map_err(|cleanup| format!("{error}. Could not remove the reserved empty duplicate: {cleanup}"))?;
            return Err(error);
        }
        let copied = io::copy(&mut input, &mut output).and_then(|bytes| {
            output.flush()?;
            output.sync_all()?;
            Ok(bytes)
        });
        drop(output);
        let source_unchanged = fs::metadata(source).is_ok_and(|after| {
            after.len() == before.len() && after.modified().ok() == before.modified().ok()
        });
        if !matches!(copied, Ok(bytes) if bytes == before.len()) || !source_unchanged {
            if let Err(error) = fs::remove_file(&destination) {
                log::error!("Could not remove incomplete duplicate: {error}");
                return Err(format!(
                    "The copy was incomplete. Remove the partial file at {}: {error}",
                    destination.display()
                ));
            }
            return Err(
                "The video changed or could not be fully copied. No duplicate was kept.".into(),
            );
        }
        if let Err(error) = fs::set_permissions(&destination, before.permissions()) {
            log::warn!("Could not copy video permissions: {error}");
        }
        return Ok(destination);
    }
    Err("Could not find an available numbered filename for the duplicate".into())
}

#[tauri::command]
pub(crate) async fn duplicate_video(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<DuplicateResult, String> {
    let file_type = fs::symlink_metadata(&path)
        .map_err(|error| format!("Cannot open video: {error}"))?
        .file_type();
    if !file_type.is_file() {
        return Err("Select a regular video file".into());
    }
    let root = selected_root(&state)?;
    let source = within_root(&path, &state)?;
    if !video_file(&source) {
        return Err("Select a supported video file".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut operation = None;
        let result = duplicate_file_with(&source, |target| {
            operation = Some(crate::catalog_operations::Operation::begin_reserved_copy(source.clone(), target.to_path_buf())?);
            Ok(())
        });
        let duplicate = match result { Ok(path) => path, Err(error) => { if let Some(operation) = operation { operation.cancel_if_unchanged(); } return Err(error); } };
        operation.ok_or("Missing duplicate catalog operation")?.finish()?;
        let folder = source.parent().ok_or("Cannot find the video's folder")?;
        Ok(DuplicateResult {
            listing: crate::catalogued_folder(folder, &root)?,
            duplicated_path: duplicate.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|error| format!("Duplicate task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    fn duplicate_file(source: &std::path::Path) -> Result<std::path::PathBuf, String> { super::duplicate_file_with(source, |_| Ok(())) }
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn duplicates_use_the_next_free_number_and_keep_contents() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "framewise-duplicate-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&folder).unwrap();
        let source = folder.join("clip.mp4");
        fs::write(&source, b"sample video bytes").unwrap();
        let first = duplicate_file(&source).unwrap();
        let second = duplicate_file(&source).unwrap();
        let third = duplicate_file(&second).unwrap();
        assert_eq!(first.file_name().unwrap(), "clip (1).mp4");
        assert_eq!(second.file_name().unwrap(), "clip (2).mp4");
        assert_eq!(third.file_name().unwrap(), "clip (3).mp4");
        assert_eq!(fs::read(&third).unwrap(), fs::read(&source).unwrap());
        for file in [first, second, third, source] {
            fs::remove_file(file).unwrap();
        }
        fs::remove_dir(folder).unwrap();
    }
}
