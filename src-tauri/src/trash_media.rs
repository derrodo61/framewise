use std::{collections::HashSet, fs, path::{Path, PathBuf}, time::UNIX_EPOCH};
use crate::media_filters::DateRange;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashFilter { expected_root: String, date: DateRange, show_videos: bool, show_images: bool, #[serde(default)] rating: crate::ratings::RatingFilter }
pub(crate) struct ValidatedBatch { files: Vec<PathBuf> }
pub(crate) struct TrashOutcome { pub moved: Vec<PathBuf>, pub remaining_paths: Vec<String>, pub error: Option<String> }

pub(crate) fn validate(root: &Path, paths: Vec<String>, filter: Option<&TrashFilter>) -> Result<ValidatedBatch, String> {
    if paths.is_empty() { return Err("Select at least one media file".into()); }
    if filter.is_some_and(|filter| filter.expected_root != root.to_string_lossy()) { return Err("Workspace changed; no files were deleted.".into()); }
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        let metadata = fs::symlink_metadata(&path).map_err(|error| format!("Cannot open file: {error}"))?;
        if !metadata.is_file() { return Err("Select regular media files".into()); }
        let resolved = Path::new(&path).canonicalize().map_err(|error| error.to_string())?;
        if !resolved.starts_with(root) || !crate::media_file(&resolved) { return Err("A selected file is outside the workspace or unsupported. No files were deleted.".into()); }
        if let Some(filter) = filter {
            let milliseconds = |time: Result<std::time::SystemTime, std::io::Error>| time.ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).and_then(|duration| i64::try_from(duration.as_millis()).ok());
            let enabled = if crate::image_file(&resolved) { filter.show_images } else { filter.show_videos };
            if !enabled || !filter.date.matches(milliseconds(metadata.modified()), milliseconds(metadata.created()))? {
                return Err("A selected file does not match the active date/type filters. No files were deleted. Refresh and select again.".into());
            }
        }
        if seen.insert(resolved.clone()) { files.push(resolved); }
    }
    if let Some(filter) = filter && !matches!(filter.rating.mode, crate::ratings::RatingMode::All) { crate::database::with_connection(|connection| crate::ratings::validate_filter_paths_on(connection, &files, &filter.rating))?; }
    Ok(ValidatedBatch { files })
}
impl ValidatedBatch {
    pub(crate) fn files(&self) -> &[PathBuf] { &self.files }
    pub(crate) fn parent(&self) -> Result<PathBuf, String> { self.files[0].parent().map(Path::to_path_buf).ok_or_else(|| "Cannot find the files' folder".into()) }
    pub(crate) fn execute(self, delete: impl FnOnce(&[PathBuf]) -> Result<(), String>) -> TrashOutcome {
        let error = delete(&self.files).err();
        let (moved, remaining): (Vec<_>, Vec<_>) = self.files.into_iter().partition(|file| matches!(fs::symlink_metadata(file), Err(cause) if cause.kind() == std::io::ErrorKind::NotFound));
        TrashOutcome { moved, remaining_paths: remaining.iter().map(|file| file.to_string_lossy().into_owned()).collect(), error }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::tests::Fixture;
    #[test]
    fn today_selection_trash_preserves_yesterday_and_stale_batch_never_executes() {
        let fixture = Fixture::new();
        let root = fixture.folder.canonicalize().unwrap();
        let today = root.join("today.jpg");
        let yesterday = root.join("yesterday.png");
        let start = 10 * 86_400_000;
        for (path, ms) in [(&today, start + 1000), (&yesterday, start - 1000)] {
            fs::write(path, b"disposable test image").unwrap();
            fs::File::options().write(true).open(path).unwrap().set_modified(UNIX_EPOCH + std::time::Duration::from_millis(ms)).unwrap();
        }
        let filter = TrashFilter { expected_root: root.to_string_lossy().into_owned(), date: DateRange { field: "modified".into(), from: Some(start as i64), to: Some((start + 86_400_000) as i64) }, show_images: true, show_videos: true, rating: crate::ratings::RatingFilter::default() };
        assert!(validate(&root, vec![today.to_string_lossy().into_owned(), yesterday.to_string_lossy().into_owned()], Some(&filter)).is_err());
        assert!(today.exists() && yesterday.exists());
        let batch = validate(&root, vec![today.to_string_lossy().into_owned()], Some(&filter)).unwrap();
        let simulated_trash = root.join("simulated-trash");
        fs::create_dir(&simulated_trash).unwrap();
        let result = batch.execute(|files| {
            for file in files {
                let target = simulated_trash.join(file.file_name().unwrap());
                assert!(file.starts_with(&root) && target.starts_with(&root));
                fs::rename(file, target).map_err(|error| error.to_string())?;
            }
            Ok(())
        });
        assert_eq!(result.moved, vec![today.clone()]);
        assert!(result.remaining_paths.is_empty());
        assert!(result.error.is_none());
        assert!(!today.exists() && yesterday.exists());
        assert!(simulated_trash.join("today.jpg").exists());
    }
}
