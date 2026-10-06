use std::{collections::{HashMap, HashSet}, path::Path, time::{SystemTime, UNIX_EPOCH}};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) struct ObservedVideo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified_ns: String,
}

pub(crate) fn register_folder(folder: &Path, present_paths: &[String], observed: &[ObservedVideo]) -> Result<HashMap<String, i64>, String> {
    crate::database::with_connection(|connection| register_folder_on(connection, folder, present_paths, observed))
}

fn register_folder_on(connection: &mut Connection, folder: &Path, present_paths: &[String], observed: &[ObservedVideo]) -> Result<HashMap<String, i64>, String> {
    let parent = folder.to_string_lossy();
    let now = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_millis()).map_err(|error| error.to_string())?;
    let present: HashSet<&str> = present_paths.iter().map(String::as_str).collect();
    for video in observed {
        if !present.contains(video.path.as_str()) || Path::new(&video.path).parent() != Some(folder) {
            return Err("Catalog discovery contains an item outside the listed folder.".into());
        }
    }
    let transaction = connection.transaction().map_err(|error| error.to_string())?;
    let mut ids = HashMap::new();
    for video in observed {
        let size = i64::try_from(video.size).map_err(|_| "Video size is too large for the catalog")?;
        let known: Option<(i64, i64, String)> = transaction.query_row(
            "SELECT id, size, modified_ns FROM videos WHERE path=?1 AND status='active'", [&video.path],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional().map_err(|error| error.to_string())?;
        let id = match known {
            Some((id, old_size, old_modified)) if old_size == size && old_modified == video.modified_ns => {
                transaction.execute("UPDATE videos SET last_seen_at=?1, name=?2 WHERE id=?3", params![now, video.name, id]).map_err(|error| error.to_string())?;
                id
            }
            other => {
                if let Some((id, _, _)) = other {
                    transaction.execute("UPDATE videos SET status='changed' WHERE id=?1", [id]).map_err(|error| error.to_string())?;
                }
                // Retained history is never automatically assigned to a replacement file.
                transaction.execute("INSERT INTO videos(path,parent_path,name,size,modified_ns,status,last_seen_at) VALUES (?1,?2,?3,?4,?5,'active',?6)",
                    params![video.path, parent, video.name, size, video.modified_ns, now]).map_err(|error| error.to_string())?;
                transaction.last_insert_rowid()
            }
        };
        ids.insert(video.path.clone(), id);
    }
    let active = {
        let mut statement = transaction.prepare("SELECT id,path FROM videos WHERE parent_path=?1 AND status='active'").map_err(|error| error.to_string())?;
        statement.query_map([parent.as_ref()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| error.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
    };
    for (id, path) in active {
        if !present.contains(path.as_str()) {
            transaction.execute("UPDATE videos SET status='missing' WHERE id=?1", [id]).map_err(|error| error.to_string())?;
        }
    }
    transaction.execute("INSERT INTO folder_discovery(path,last_completed_at) VALUES (?1,?2) ON CONFLICT(path) DO UPDATE SET last_completed_at=excluded.last_completed_at", params![parent, now]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{open_at, tests::Fixture};

    fn observation(folder: &Path, name: &str, version: &str) -> ObservedVideo {
        ObservedVideo { path: folder.join(name).to_string_lossy().into_owned(), name: name.into(), size: 100, modified_ns: version.into() }
    }
    fn register(connection: &mut Connection, folder: &Path, observed: &[ObservedVideo]) -> HashMap<String, i64> {
        let present = observed.iter().map(|video| video.path.clone()).collect::<Vec<_>>();
        register_folder_on(connection, folder, &present, observed).unwrap()
    }
    fn status(connection: &Connection, id: i64) -> String {
        connection.query_row("SELECT status FROM videos WHERE id=?1", [id], |row| row.get(0)).unwrap()
    }

    #[test]
    fn repeated_discovery_and_reopening_preserve_ids_and_assignments() {
        let fixture = Fixture::new();
        let mut connection = open_at(&fixture.path).unwrap();
        let video = observation(&fixture.folder, "clip.mp4", "123456789");
        let id = register(&mut connection, &fixture.folder, std::slice::from_ref(&video))[&video.path];
        connection.execute("INSERT INTO tags(name,normalized_name) VALUES ('Keeper','keeper')", []).unwrap();
        connection.execute("INSERT INTO video_tags VALUES (?1,1)", [id]).unwrap();
        assert!(connection.execute("INSERT INTO video_tags VALUES (?1,1)", [id]).is_err());
        assert_eq!(register(&mut connection, &fixture.folder, std::slice::from_ref(&video))[&video.path], id);
        drop(connection);
        let mut reopened = open_at(&fixture.path).unwrap();
        assert_eq!(register(&mut reopened, &fixture.folder, &[video])[&fixture.folder.join("clip.mp4").to_string_lossy().into_owned()], id);
        assert_eq!(reopened.query_row("SELECT video_id FROM video_tags", [], |row| row.get::<_, i64>(0)).unwrap(), id);
        // Preview cleanup does not own or delete catalog records or tags.
        reopened.execute("DELETE FROM previews", []).unwrap();
        assert_eq!(reopened.query_row("SELECT COUNT(*) FROM video_tags", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        reopened.execute("DELETE FROM tags", []).unwrap();
        assert_eq!(reopened.query_row("SELECT COUNT(*) FROM video_tags", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(status(&reopened, id), "active");
    }

    #[test]
    fn replacements_and_reappearing_paths_do_not_inherit_old_tags() {
        let fixture = Fixture::new();
        let mut connection = open_at(&fixture.path).unwrap();
        let mut video = observation(&fixture.folder, "clip.mp4", "100");
        let first = register(&mut connection, &fixture.folder, std::slice::from_ref(&video))[&video.path];
        connection.execute("INSERT INTO tags(name,normalized_name) VALUES ('Original','original')", []).unwrap();
        connection.execute("INSERT INTO video_tags VALUES (?1,1)", [first]).unwrap();
        video.modified_ns = "101".into();
        let replacement = register(&mut connection, &fixture.folder, std::slice::from_ref(&video))[&video.path];
        assert_ne!(first, replacement);
        assert_eq!(status(&connection, first), "changed");
        assert_eq!(connection.query_row("SELECT video_id FROM video_tags", [], |row| row.get::<_, i64>(0)).unwrap(), first);
        register(&mut connection, &fixture.folder, &[]);
        assert_eq!(status(&connection, replacement), "missing");
        let reappeared = register(&mut connection, &fixture.folder, std::slice::from_ref(&video))[&video.path];
        assert_ne!(reappeared, replacement);
        assert_eq!(status(&connection, replacement), "missing");
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM video_tags WHERE video_id=?1", [reappeared], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn missing_metadata_and_other_folder_scans_do_not_mark_videos_missing() {
        let fixture = Fixture::new();
        let mut connection = open_at(&fixture.path).unwrap();
        let video = observation(&fixture.folder, "clip.mp4", "100");
        let id = register(&mut connection, &fixture.folder, std::slice::from_ref(&video))[&video.path];
        register_folder_on(&mut connection, &fixture.folder, std::slice::from_ref(&video.path), &[]).unwrap();
        register(&mut connection, &fixture.folder.join("different"), &[]);
        assert_eq!(status(&connection, id), "active");
        assert!(register_folder_on(&mut connection, &fixture.folder.join("different"), std::slice::from_ref(&video.path), std::slice::from_ref(&video)).is_err());
        assert_eq!(status(&connection, id), "active");
    }

    #[test]
    fn invalid_batch_rolls_back_discovery() {
        let fixture = Fixture::new();
        let mut connection = open_at(&fixture.path).unwrap();
        let first = observation(&fixture.folder, "first.mp4", "100");
        let mut invalid = observation(&fixture.folder, "second.mp4", "100");
        invalid.size = u64::MAX;
        let present = vec![first.path.clone(), invalid.path.clone()];
        assert!(register_folder_on(&mut connection, &fixture.folder, &present, &[first, invalid]).is_err());
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM videos", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM folder_discovery", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }
}
