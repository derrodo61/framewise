use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

use crate::database::with_connection;
pub(crate) use crate::database::database_path;

pub(crate) struct Entry {
    pub version: String,
    pub image_name: String,
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn find_on(connection: &Connection, video: &Path) -> rusqlite::Result<Option<Entry>> {
    connection.query_row(
        "SELECT version, image_name FROM previews WHERE video_path = ?1",
        [path_text(video)],
        |row| Ok(Entry { version: row.get(0)?, image_name: row.get(1)? }),
    ).optional()
}

pub(crate) fn find(video: &Path) -> Result<Option<Entry>, String> {
    with_connection(|connection| find_on(connection, video)
        .map_err(|error| format!("Cannot read preview index: {error}")))
}

pub(crate) fn record(video: &Path, version: &str, image_name: &str) -> Result<Option<String>, String> {
    with_connection(|connection| record_on(connection, video, version, image_name))
}

fn record_on(connection: &mut Connection, video: &Path, version: &str, image_name: &str) -> Result<Option<String>, String> {
    let transaction = connection.transaction()
        .map_err(|error| format!("Cannot update preview index: {error}"))?;
    let previous = find_on(&transaction, video)
        .map_err(|error| format!("Cannot read preview index: {error}"))?;
    transaction.execute(
        "INSERT INTO previews (video_path, version, image_name) VALUES (?1, ?2, ?3)
         ON CONFLICT(video_path) DO UPDATE SET version = excluded.version, image_name = excluded.image_name",
        params![path_text(video), version, image_name],
    ).map_err(|error| format!("Cannot save preview index: {error}"))?;
    transaction.commit().map_err(|error| format!("Cannot save preview index: {error}"))?;
    Ok(previous.map(|entry| entry.image_name).filter(|old| old != image_name))
}

pub(crate) fn forget(video: &Path) -> Result<Option<String>, String> {
    with_connection(|connection| forget_on(connection, video))
}

fn forget_on(connection: &mut Connection, video: &Path) -> Result<Option<String>, String> {
    let transaction = connection.transaction()
        .map_err(|error| format!("Cannot update preview index: {error}"))?;
    let previous = find_on(&transaction, video)
        .map_err(|error| format!("Cannot read preview index: {error}"))?;
    transaction.execute("DELETE FROM previews WHERE video_path = ?1", [path_text(video)])
        .map_err(|error| format!("Cannot remove preview index entry: {error}"))?;
    transaction.commit().map_err(|error| format!("Cannot remove preview index entry: {error}"))?;
    Ok(previous.map(|entry| entry.image_name))
}

pub(crate) fn relocate(old: &Path, new: &Path) -> Result<Option<String>, String> {
    with_connection(|connection| relocate_on(connection, old, new))
}

fn relocate_on(connection: &mut Connection, old: &Path, new: &Path) -> Result<Option<String>, String> {
    let transaction = connection.transaction()
        .map_err(|error| format!("Cannot update preview index: {error}"))?;
    let replaced = find_on(&transaction, new)
        .map_err(|error| format!("Cannot read preview index: {error}"))?;
    transaction.execute("DELETE FROM previews WHERE video_path = ?1", [path_text(new)])
        .map_err(|error| format!("Cannot update preview index: {error}"))?;
    transaction.execute("UPDATE previews SET video_path = ?1 WHERE video_path = ?2", params![path_text(new), path_text(old)])
        .map_err(|error| format!("Cannot update preview index: {error}"))?;
    transaction.commit().map_err(|error| format!("Cannot update preview index: {error}"))?;
    Ok(replaced.map(|entry| entry.image_name))
}

fn entries_under(connection: &Connection, folder: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let mut statement = connection.prepare("SELECT video_path, image_name FROM previews")
        .map_err(|error| format!("Cannot read preview index: {error}"))?;
    let rows = statement.query_map([], |row| Ok((PathBuf::from(row.get::<_, String>(0)?), row.get::<_, String>(1)?)))
        .map_err(|error| format!("Cannot read preview index: {error}"))?;
    rows.map(|row| row.map_err(|error| format!("Cannot read preview index: {error}")))
        .filter(|row| match row { Ok((path, _)) => path.starts_with(folder), Err(_) => true })
        .collect()
}

pub(crate) fn forget_folder(folder: &Path) -> Result<Vec<String>, String> {
    with_connection(|connection| forget_folder_on(connection, folder))
}

fn forget_folder_on(connection: &mut Connection, folder: &Path) -> Result<Vec<String>, String> {
    let entries = entries_under(connection, folder)?;
    let transaction = connection.transaction()
        .map_err(|error| format!("Cannot update preview index: {error}"))?;
    for (path, _) in &entries {
        transaction.execute("DELETE FROM previews WHERE video_path = ?1", [path_text(path)])
            .map_err(|error| format!("Cannot remove preview index entries: {error}"))?;
    }
    transaction.commit().map_err(|error| format!("Cannot remove preview index entries: {error}"))?;
    Ok(entries.into_iter().map(|(_, image)| image).collect())
}

pub(crate) fn relocate_folder(old: &Path, new: &Path) -> Result<Vec<String>, String> {
    with_connection(|connection| relocate_folder_on(connection, old, new))
}

fn relocate_folder_on(connection: &mut Connection, old: &Path, new: &Path) -> Result<Vec<String>, String> {
    let entries = entries_under(connection, old)?;
    let transaction = connection.transaction()
        .map_err(|error| format!("Cannot update preview index: {error}"))?;
    let mut displaced = Vec::new();
    for (source, _) in entries {
        let target = new.join(source.strip_prefix(old).map_err(|error| error.to_string())?);
        if let Some(entry) = find_on(&transaction, &target)
            .map_err(|error| format!("Cannot read preview index: {error}"))?
        {
            displaced.push(entry.image_name);
        }
        transaction.execute("DELETE FROM previews WHERE video_path = ?1", [path_text(&target)])
            .map_err(|error| format!("Cannot update preview index: {error}"))?;
        transaction.execute("UPDATE previews SET video_path = ?1 WHERE video_path = ?2", params![path_text(&target), path_text(&source)])
            .map_err(|error| format!("Cannot update preview index: {error}"))?;
    }
    transaction.commit().map_err(|error| format!("Cannot update preview index: {error}"))?;
    Ok(displaced)
}

#[cfg(test)]
mod tests {
    use super::{find_on, forget_folder_on, forget_on, record_on, relocate_folder_on, relocate_on};
    use crate::database::open_at;
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn preview_index_tracks_versions_moves_and_deletions() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let folder = std::env::temp_dir().join(format!("framewise-preview-index-{}-{stamp}", std::process::id()));
        let mut connection = open_at(&folder.join("framewise.db")).unwrap();
        let source = folder.join("media").join("clip.mp4");
        let renamed = folder.join("media").join("renamed.mp4");
        let destination = folder.join("moved").join("renamed.mp4");
        let old_image = "1111111111111111.png";
        let new_image = "2222222222222222.png";

        assert!(record_on(&mut connection, &source, "version-one", old_image).unwrap().is_none());
        assert_eq!(find_on(&connection, &source).unwrap().unwrap().image_name, old_image);
        assert_eq!(record_on(&mut connection, &source, "version-two", new_image).unwrap().as_deref(), Some(old_image));
        assert!(relocate_on(&mut connection, &source, &renamed).unwrap().is_none());
        assert!(find_on(&connection, &source).unwrap().is_none());
        assert_eq!(find_on(&connection, &renamed).unwrap().unwrap().version, "version-two");
        assert!(relocate_folder_on(&mut connection, &folder.join("media"), &folder.join("moved")).unwrap().is_empty());
        assert!(find_on(&connection, &renamed).unwrap().is_none());
        assert_eq!(find_on(&connection, &destination).unwrap().unwrap().image_name, new_image);
        assert_eq!(forget_folder_on(&mut connection, &folder.join("moved")).unwrap(), vec![new_image]);
        assert!(find_on(&connection, &destination).unwrap().is_none());
        assert!(forget_on(&mut connection, &source).unwrap().is_none());

        drop(connection);
        fs::remove_dir_all(folder).unwrap();
    }
}
