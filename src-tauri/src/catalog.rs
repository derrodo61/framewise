use std::{collections::{HashMap, HashSet}, path::Path, time::{SystemTime, UNIX_EPOCH}};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ObservedVideo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified_ns: String,
}

pub(crate) fn observe(path: &Path) -> Result<ObservedVideo, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    if !metadata.is_file() { return Err("Catalog operations require a regular video file.".into()); }
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    Ok(ObservedVideo {
        path: path.to_string_lossy().into_owned(),
        name: path.file_name().ok_or("Cannot read filename")?.to_string_lossy().into_owned(),
        size: metadata.len(),
        modified_ns: metadata.modified().map_err(|error| error.to_string())?.duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos().to_string(),
    })
}

pub(crate) fn content_hash(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let before = observe(path)?;
    let mut input = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65_536];
    loop { let count = input.read(&mut buffer).map_err(|error| error.to_string())?; if count == 0 { break } hash.update(&buffer[..count]); }
    if observe(path)? != before { return Err("Video changed while checking its content identity.".into()); }
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn ensure_observed_on(connection: &Connection, video: &ObservedVideo) -> Result<i64, String> {
    let size = i64::try_from(video.size).map_err(|error| error.to_string())?;
    let known: Option<(i64, i64, String)> = connection.query_row("SELECT id,size,modified_ns FROM videos WHERE path=?1 AND status='active'", [&video.path], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .optional().map_err(|error| error.to_string())?;
    if let Some((id, old_size, modified)) = &known
        && *old_size == size && modified == &video.modified_ns { return Ok(*id); }
    if let Some((id, _, _)) = known { connection.execute("UPDATE videos SET status='changed' WHERE id=?1", [id]).map_err(|error| error.to_string())?; }
    let candidates = {
        let mut statement = connection.prepare("SELECT id,content_hash FROM videos WHERE path=?1 AND size=?2 AND status='trashed' AND content_hash IS NOT NULL").map_err(|error| error.to_string())?;
        statement.query_map(params![video.path, size], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| error.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
    };
    if !candidates.is_empty() {
        let hash = content_hash(Path::new(&video.path))?;
        let matches: Vec<_> = candidates.iter().filter(|(_, candidate)| *candidate == hash).collect();
        if matches.len() == 1 {
            let id = matches[0].0;
            connection.execute("UPDATE videos SET status='active',modified_ns=?1,last_seen_at=unixepoch()*1000 WHERE id=?2", params![video.modified_ns, id]).map_err(|error| error.to_string())?;
            return Ok(id);
        }
    }
    let parent = Path::new(&video.path).parent().ok_or("Cannot read parent folder")?.to_string_lossy();
    let kind = if crate::image_file(Path::new(&video.path)) { "image" } else { "video" };
    connection.execute("INSERT INTO videos(path,parent_path,name,size,modified_ns,status,last_seen_at,media_kind) VALUES (?1,?2,?3,?4,?5,'active',unixepoch()*1000,?6)", params![video.path, parent, video.name, size, video.modified_ns, kind]).map_err(|error| error.to_string())?;
    Ok(connection.last_insert_rowid())
}

pub(crate) fn register_folder(folder: &Path, present_paths: &[String], observed: &[ObservedVideo]) -> Result<HashMap<String, i64>, String> {
    crate::database::with_connection(|connection| register_folder_on(connection, folder, present_paths, observed))
}

pub(crate) fn register_folder_on(connection: &mut Connection, folder: &Path, present_paths: &[String], observed: &[ObservedVideo]) -> Result<HashMap<String, i64>, String> {
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
        let id = ensure_observed_on(&transaction, video)?;
        transaction.execute("UPDATE videos SET last_seen_at=?1, name=?2 WHERE id=?3", params![now, video.name, id]).map_err(|error| error.to_string())?;
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
