use std::{collections::HashMap, path::Path};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize)]
pub(crate) struct RatingFilter {
    #[serde(default)]
    pub mode: RatingMode,
    #[serde(default = "default_value")]
    pub value: u8,
}
fn default_value() -> u8 { 3 }
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum RatingMode { #[default] All, Unrated, Exactly, AtLeast, MoreThan }
impl RatingFilter {
    pub(crate) fn sql(&self) -> Result<String, String> {
        // Interpolation is restricted to an enum and a validated integer.
        Ok(match self.mode {
            RatingMode::All => "1".into(),
            RatingMode::Unrated => "v.rating IS NULL".into(),
            _ => {
                if !(1..=5).contains(&self.value) { return Err("Choose a rating from 1 to 5.".into()); }
                let operator = match self.mode { RatingMode::Exactly => "=", RatingMode::AtLeast => ">=", RatingMode::MoreThan => ">", _ => unreachable!() };
                format!("v.rating {operator} {}", self.value)
            }
        })
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MediaRating { video_id: i64, rating: Option<u8> }
pub(crate) fn folder_ratings_on(connection: &Connection, folder: &Path) -> Result<HashMap<i64, u8>, String> {
    let mut statement = connection.prepare("SELECT id,rating FROM videos WHERE parent_path=?1 AND status='active' AND rating IS NOT NULL").map_err(|error| error.to_string())?;
    statement.query_map([folder.to_string_lossy()], |row| Ok((row.get(0)?, row.get(1)?))).map_err(|error| error.to_string())?
        .collect::<Result<_, _>>().map_err(|error| error.to_string())
}
fn set_on(connection: &mut Connection, root: &Path, videos: &[crate::tags::TagVideo], rating: Option<u8>) -> Result<Vec<MediaRating>, String> {
    if rating.is_some_and(|value| !(1..=5).contains(&value)) { return Err("Choose 1–5 stars or clear the rating.".into()); }
    let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|error| error.to_string())?;
    let ids = crate::tags::validate_videos(&transaction, root, videos)?;
    for id in &ids { transaction.execute("UPDATE videos SET rating=?1 WHERE id=?2", params![rating, id]).map_err(|error| error.to_string())?; }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(ids.into_iter().map(|video_id| MediaRating { video_id, rating }).collect())
}
#[tauri::command]
pub(crate) async fn set_media_rating(videos: Vec<crate::tags::TagVideo>, rating: Option<u8>, state: tauri::State<'_, crate::AppState>) -> Result<Vec<MediaRating>, String> {
    let root = crate::selected_root(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::catalog_operations::recover_pending()?;
        crate::database::with_connection(|connection| set_on(connection, &root, &videos, rating))
    }).await.map_err(|error| error.to_string())?
}
pub(crate) fn validate_filter_paths_on(connection: &Connection, paths: &[std::path::PathBuf], filter: &RatingFilter) -> Result<(), String> {
    let predicate = filter.sql()?;
    if matches!(filter.mode, RatingMode::All) { return Ok(()); }
    for path in paths {
        let matches = connection.query_row(&format!("SELECT ({predicate}) FROM videos v WHERE path=?1 AND status='active'"), [path.to_string_lossy()], |row| row.get::<_, Option<bool>>(0))
            .optional().map_err(|error| error.to_string())?.flatten().unwrap_or(false);
        if !matches { return Err("A selected file no longer matches the rating filter. No files were deleted. Refresh and select again.".into()); }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{open_at, tests::Fixture};
    #[test]
    fn ratings_are_atomic_scoped_persistent_and_clearable() {
        let fixture = Fixture::new(); let root = fixture.folder.canonicalize().unwrap();
        let mut connection = open_at(&fixture.path).unwrap();
        let mut files = Vec::new();
        for name in ["clip.mp4", "image.png"] {
            let path = root.join(name); std::fs::write(&path, b"media").unwrap();
            let observed = crate::catalog::observe(&path).unwrap();
            let video_id = crate::catalog::ensure_observed_on(&connection, &observed).unwrap();
            files.push(crate::tags::TagVideo { video_id, path: observed.path });
        }
        set_on(&mut connection, &root, &files, Some(4)).unwrap();
        assert_eq!(folder_ratings_on(&connection, &root).unwrap().len(), 2);
        assert!(set_on(&mut connection, &root, &files, Some(0)).is_err());
        assert!(set_on(&mut connection, &root.join("outside"), &files, Some(5)).is_err());
        let paths: Vec<_> = files.iter().map(|file| std::path::PathBuf::from(&file.path)).collect();
        let filter = RatingFilter { mode: RatingMode::MoreThan, value: 3 };
        assert!(validate_filter_paths_on(&connection, &paths, &filter).is_ok());
        std::fs::write(&files[1].path, b"changed content").unwrap();
        assert!(set_on(&mut connection, &root, &files, Some(2)).is_err());
        assert_eq!(folder_ratings_on(&connection, &root).unwrap().get(&files[0].video_id), Some(&4));
        set_on(&mut connection, &root, &files[..1], None).unwrap();
        assert!(validate_filter_paths_on(&connection, &paths, &filter).is_err());
        assert!(validate_filter_paths_on(&connection, &paths[..1], &RatingFilter { mode: RatingMode::Unrated, value: 3 }).is_ok());
        drop(connection); let reopened = open_at(&fixture.path).unwrap();
        assert_eq!(folder_ratings_on(&reopened, &root).unwrap().get(&files[1].video_id), Some(&4));
        assert_eq!(folder_ratings_on(&reopened, &root).unwrap().get(&files[0].video_id), None);
    }
}
