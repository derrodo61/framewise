use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use crate::database::{normalize_tag_name, tag_search_key, with_connection};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Tag { id: i64, name: String, video_count: i64 }

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TagVideo { video_id: i64, path: String }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SelectionTag { id: i64, name: String, assigned_count: i64 }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TagSelection { video_count: usize, tags: Vec<SelectionTag> }

fn validate_videos(connection: &Connection, root: &std::path::Path, videos: &[TagVideo]) -> Result<Vec<i64>, String> {
    if videos.is_empty() { return Err("Select at least one video to tag.".into()); }
    let mut ids = std::collections::HashSet::new();
    let mut paths = Vec::new();
    for video in videos {
        let observed = crate::catalog::observe(std::path::Path::new(&video.path))?;
        let path = std::path::PathBuf::from(&observed.path);
        if !path.starts_with(root) || !crate::video_file(&path) { return Err("A selected video is outside the workspace or is not supported.".into()); }
        let matches: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM videos WHERE id=?1 AND path=?2 AND size=?3 AND modified_ns=?4 AND status='active')",
            params![video.video_id, observed.path, i64::try_from(observed.size).map_err(|error| error.to_string())?, observed.modified_ns], |row| row.get(0)).map_err(|error| error.to_string())?;
        if !matches { return Err("A selected video changed or moved. Refresh the folder and select it again before editing tags.".into()); }
        ids.insert(video.video_id); paths.push(path);
    }
    crate::catalog_operations::ensure_paths_idle(&paths)?;
    Ok(ids.into_iter().collect())
}
fn selection_on(connection: &Connection, ids: &[i64]) -> Result<TagSelection, String> {
    connection.execute_batch("CREATE TEMP TABLE IF NOT EXISTS framewise_tag_selection(video_id INTEGER PRIMARY KEY); DELETE FROM framewise_tag_selection;").map_err(|error| error.to_string())?;
    for id in ids { connection.execute("INSERT INTO framewise_tag_selection VALUES (?1)", [id]).map_err(|error| error.to_string())?; }
    let mut statement = connection.prepare("SELECT t.id,t.name,(SELECT COUNT(*) FROM video_tags vt JOIN framewise_tag_selection s ON s.video_id=vt.video_id WHERE vt.tag_id=t.id) FROM tags t ORDER BY normalized_name,id").map_err(|error| error.to_string())?;
    let tags = statement.query_map([], |row| Ok(SelectionTag { id: row.get(0)?, name: row.get(1)?, assigned_count: row.get(2)? }))
        .map_err(|error| error.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    Ok(TagSelection { video_count: ids.len(), tags })
}
enum Assignment { Read, Set { tag_id: i64, assigned: bool }, Create { name: String } }
fn assignment_on(connection: &mut Connection, root: &std::path::Path, videos: &[TagVideo], action: Assignment) -> Result<TagSelection, String> {
    let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|error| error.to_string())?;
    let ids = validate_videos(&transaction, root, videos)?;
    let mutation = match action {
        Assignment::Read => None,
        Assignment::Set { tag_id, assigned } => {
            let exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM tags WHERE id=?1)", [tag_id], |row| row.get(0)).map_err(|error| error.to_string())?;
            if !exists { return Err("This tag no longer exists. Refresh the tags and try again.".into()); }
            Some((tag_id, assigned))
        }
        Assignment::Create { name } => {
            let (display, key) = normalize_tag_name(&name)?;
            transaction.execute("INSERT INTO tags(name,normalized_name) VALUES (?1,?2) ON CONFLICT(normalized_name) DO NOTHING", params![display, key]).map_err(name_error)?;
            let id = transaction.query_row("SELECT id FROM tags WHERE normalized_name=?1", [key], |row| row.get::<_, i64>(0)).map_err(|error| error.to_string())?;
            Some((id, true))
        }
    };
    if let Some((tag, assigned)) = mutation {
        for video in &ids {
            if assigned { transaction.execute("INSERT INTO video_tags(video_id,tag_id) VALUES (?1,?2) ON CONFLICT DO NOTHING", params![video, tag]) }
            else { transaction.execute("DELETE FROM video_tags WHERE video_id=?1 AND tag_id=?2", params![video, tag]) }.map_err(|error| error.to_string())?;
        }
    }
    let result = selection_on(&transaction, &ids)?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(result)
}
async fn assignment(root: std::path::PathBuf, videos: Vec<TagVideo>, action: Assignment) -> Result<TagSelection, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::catalog_operations::recover_pending()?;
        with_connection(|connection| assignment_on(connection, &root, &videos, action))
    }).await.map_err(|error| error.to_string())?
}
#[tauri::command]
pub(crate) async fn selection_tags(videos: Vec<TagVideo>, state: tauri::State<'_, crate::AppState>) -> Result<TagSelection, String> {
    assignment(crate::selected_root(&state)?, videos, Assignment::Read).await
}
#[tauri::command]
pub(crate) async fn set_video_tag(videos: Vec<TagVideo>, tag_id: i64, assigned: bool, state: tauri::State<'_, crate::AppState>) -> Result<TagSelection, String> {
    assignment(crate::selected_root(&state)?, videos, Assignment::Set { tag_id, assigned }).await
}
#[tauri::command]
pub(crate) async fn create_and_assign_tag(videos: Vec<TagVideo>, name: String, state: tauri::State<'_, crate::AppState>) -> Result<TagSelection, String> {
    assignment(crate::selected_root(&state)?, videos, Assignment::Create { name }).await
}

fn list_on(connection: &Connection, query: &str) -> Result<Vec<Tag>, String> {
    let key = tag_search_key(query);
    let mut statement = connection.prepare("SELECT t.id,t.name,COUNT(vt.video_id) FROM tags t
        LEFT JOIN video_tags vt ON vt.tag_id=t.id WHERE instr(t.normalized_name,?1)>0 OR ?1=''
        GROUP BY t.id ORDER BY t.normalized_name,t.id").map_err(|error| error.to_string())?;
    statement.query_map([key], |row| Ok(Tag { id: row.get(0)?, name: row.get(1)?, video_count: row.get(2)? }))
        .map_err(|error| error.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
}
fn name_error(error: rusqlite::Error) -> String {
    if matches!(&error, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::ConstraintViolation) {
        "A tag with this name already exists. Choose a different name.".into()
    } else { format!("Could not save tag: {error}") }
}
fn create_on(connection: &Connection, name: &str) -> Result<(), String> {
    let (display, key) = normalize_tag_name(name)?;
    connection.execute("INSERT INTO tags(name,normalized_name) VALUES (?1,?2)", params![display, key]).map_err(name_error)?;
    Ok(())
}
fn rename_on(connection: &Connection, id: i64, name: &str) -> Result<(), String> {
    let (display, key) = normalize_tag_name(name)?;
    let count = connection.execute("UPDATE tags SET name=?1,normalized_name=?2 WHERE id=?3", params![display, key, id]).map_err(name_error)?;
    if count == 0 { return Err("This tag no longer exists. Refresh the tag list.".into()); }
    Ok(())
}
fn delete_on(connection: &mut Connection, id: i64, expected_name: &str, expected_video_count: i64) -> Result<(), String> {
    let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|error| error.to_string())?;
    let tag: Option<(String, i64)> = transaction.query_row("SELECT t.name,COUNT(vt.video_id) FROM tags t LEFT JOIN video_tags vt ON vt.tag_id=t.id WHERE t.id=?1 GROUP BY t.id", [id], |row| Ok((row.get(0)?, row.get(1)?)))
        .optional().map_err(|error| error.to_string())?;
    let Some((name, count)) = tag else { return Err("This tag no longer exists. Refresh the tag list.".into()); };
    if name != expected_name || count != expected_video_count {
        return Err("This tag or its video assignments changed while confirmation was open. Review the refreshed list and try again.".into());
    }
    transaction.execute("DELETE FROM tags WHERE id=?1", [id]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn list_tags(query: Option<String>) -> Result<Vec<Tag>, String> {
    tauri::async_runtime::spawn_blocking(move || with_connection(|connection| list_on(connection, query.as_deref().unwrap_or(""))))
        .await.map_err(|error| error.to_string())?
}
#[tauri::command]
pub(crate) async fn create_tag(name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || with_connection(|connection| create_on(connection, &name)))
        .await.map_err(|error| error.to_string())?
}
#[tauri::command]
pub(crate) async fn rename_tag(tag_id: i64, name: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || with_connection(|connection| rename_on(connection, tag_id, &name)))
        .await.map_err(|error| error.to_string())?
}
#[tauri::command]
pub(crate) async fn delete_tag(tag_id: i64, expected_name: String, expected_video_count: i64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || with_connection(|connection| delete_on(connection, tag_id, &expected_name, expected_video_count)))
        .await.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{open_at, tests::Fixture};
    #[test]
    fn names_search_and_persistence_use_unicode_normalization() {
        let fixture = Fixture::new();
        let connection = open_at(&fixture.path).unwrap();
        create_on(&connection, "  Straße  ").unwrap();
        create_on(&connection, "Café").unwrap();
        assert!(create_on(&connection, "STRASSE").unwrap_err().contains("already exists"));
        assert!(create_on(&connection, "Cafe\u{301}").is_err());
        assert!(create_on(&connection, " ").is_err());
        assert!(create_on(&connection, "a\nb").is_err());
        assert!(create_on(&connection, &"x".repeat(101)).is_err());
        assert_eq!(list_on(&connection, "strasse").unwrap()[0].name, "Straße");
        assert_eq!(list_on(&connection, "Cafe\u{301}").unwrap()[0].name, "Café");
        assert!(list_on(&connection, "missing").unwrap().is_empty());
        assert!(list_on(&connection, &"x".repeat(101)).unwrap().is_empty());
        drop(connection);
        let reopened = open_at(&fixture.path).unwrap();
        assert_eq!(list_on(&reopened, "").unwrap().len(), 2);
    }
    #[test]
    fn rename_and_delete_keep_video_records_and_require_current_confirmation() {
        let fixture = Fixture::new();
        let mut connection = open_at(&fixture.path).unwrap();
        create_on(&connection, "Keeper").unwrap();
        let id = list_on(&connection, "").unwrap()[0].id;
        connection.execute_batch("INSERT INTO videos(id,path,parent_path,name,size,modified_ns,status,last_seen_at) VALUES (9,'clip.mp4','.','clip.mp4',100,'123','active',1);").unwrap();
        connection.execute("INSERT INTO video_tags VALUES (9,?1)", [id]).unwrap();
        rename_on(&connection, id, "Favorite").unwrap();
        let tags = list_on(&connection, "favorite").unwrap();
        assert_eq!(tags[0].id, id);
        assert_eq!(tags[0].video_count, 1);
        assert!(delete_on(&mut connection, id, "Keeper", 1).is_err());
        assert!(delete_on(&mut connection, id, "Favorite", 0).is_err());
        assert_eq!(list_on(&connection, "").unwrap().len(), 1);
        delete_on(&mut connection, id, "Favorite", 1).unwrap();
        assert!(list_on(&connection, "").unwrap().is_empty());
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM video_tags", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM videos WHERE id=9", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
        assert!(rename_on(&connection, id, "Gone").is_err());
        assert!(delete_on(&mut connection, id, "Favorite", 1).is_err());
    }
    #[test]
    fn rename_collision_is_atomic_and_display_case_can_change() {
        let fixture = Fixture::new();
        let connection = open_at(&fixture.path).unwrap();
        create_on(&connection, "Landscape").unwrap();
        create_on(&connection, "Keeper").unwrap();
        let id = list_on(&connection, "keeper").unwrap()[0].id;
        assert!(rename_on(&connection, id, "LANDSCAPE").is_err());
        assert_eq!(list_on(&connection, "keeper").unwrap()[0].name, "Keeper");
        rename_on(&connection, id, "KEEPER").unwrap();
        assert_eq!(list_on(&connection, "keeper").unwrap()[0].name, "KEEPER");
    }

    fn seed_video(connection: &Connection, fixture: &Fixture, name: &str) -> TagVideo {
        let path = fixture.folder.join(name);
        std::fs::write(&path, b"sample video").unwrap();
        let video = crate::catalog::observe(&path).unwrap();
        let id = crate::catalog::ensure_observed_on(connection, &video).unwrap();
        TagVideo { video_id: id, path: video.path }
    }
    fn assigned(result: &TagSelection, name: &str) -> i64 { result.tags.iter().find(|tag| tag.name == name).unwrap().assigned_count }

    #[test]
    fn single_batch_and_partial_assignments_are_idempotent_and_scoped() {
        let fixture = Fixture::new();
        let mut connection = open_at(&fixture.path).unwrap();
        let root = fixture.folder.canonicalize().unwrap();
        let first = seed_video(&connection, &fixture, "first.mp4");
        let second = seed_video(&connection, &fixture, "second.mp4");
        let other = seed_video(&connection, &fixture, "other.mp4");
        create_on(&connection, "Keeper").unwrap();
        create_on(&connection, "Landscape").unwrap();
        let keeper = list_on(&connection, "keeper").unwrap()[0].id;
        let landscape = list_on(&connection, "landscape").unwrap()[0].id;
        assignment_on(&mut connection, &root, std::slice::from_ref(&first), Assignment::Set { tag_id: keeper, assigned: true }).unwrap();
        assignment_on(&mut connection, &root, std::slice::from_ref(&other), Assignment::Set { tag_id: keeper, assigned: true }).unwrap();
        assignment_on(&mut connection, &root, std::slice::from_ref(&first), Assignment::Set { tag_id: landscape, assigned: true }).unwrap();
        let pair = [first.clone(), second.clone()];
        let partial = assignment_on(&mut connection, &root, &pair, Assignment::Read).unwrap();
        assert_eq!(partial.video_count, 2);
        assert_eq!(assigned(&partial, "Keeper"), 1);
        let all = assignment_on(&mut connection, &root, &pair, Assignment::Set { tag_id: keeper, assigned: true }).unwrap();
        assert_eq!(assigned(&all, "Keeper"), 2);
        let repeated = assignment_on(&mut connection, &root, &pair, Assignment::Set { tag_id: keeper, assigned: true }).unwrap();
        assert_eq!(assigned(&repeated, "Keeper"), 2);
        let removed = assignment_on(&mut connection, &root, &pair, Assignment::Set { tag_id: keeper, assigned: false }).unwrap();
        assert_eq!(assigned(&removed, "Keeper"), 0);
        assert_eq!(assigned(&removed, "Landscape"), 1);
        assert_eq!(assigned(&assignment_on(&mut connection, &root, &[other], Assignment::Read).unwrap(), "Keeper"), 1);
        let deduplicated = assignment_on(&mut connection, &root, &[first.clone(), first], Assignment::Read).unwrap();
        assert_eq!(deduplicated.video_count, 1);
        assert_eq!(assigned(&deduplicated, "Landscape"), 1);
    }

    #[test]
    fn create_and_assign_reuses_unicode_names_and_persists() {
        let fixture = Fixture::new();
        let mut connection = open_at(&fixture.path).unwrap();
        let root = fixture.folder.canonicalize().unwrap();
        let first = seed_video(&connection, &fixture, "first.mp4");
        let second = seed_video(&connection, &fixture, "second.mp4");
        let initial = assignment_on(&mut connection, &root, std::slice::from_ref(&first), Assignment::Create { name: "  Straße  ".into() }).unwrap();
        assert_eq!(assigned(&initial, "Straße"), 1);
        let pair = [first, second];
        let reused = assignment_on(&mut connection, &root, &pair, Assignment::Create { name: "STRASSE".into() }).unwrap();
        assert_eq!(reused.tags.len(), 1);
        assert_eq!(assigned(&reused, "Straße"), 2);
        let id = reused.tags[0].id;
        rename_on(&connection, id, "Travel").unwrap();
        drop(connection);
        let mut reopened = open_at(&fixture.path).unwrap();
        let persisted = assignment_on(&mut reopened, &root, &pair, Assignment::Read).unwrap();
        assert_eq!(persisted.tags[0].id, id);
        assert_eq!(assigned(&persisted, "Travel"), 2);
        delete_on(&mut reopened, id, "Travel", 2).unwrap();
        assert!(assignment_on(&mut reopened, &root, &pair, Assignment::Read).unwrap().tags.is_empty());
        assert!(std::path::Path::new(&pair[0].path).is_file());
    }

    #[test]
    fn stale_missing_outside_and_invalid_batches_do_not_write_assignments() {
        let fixture = Fixture::new();
        let mut connection = open_at(&fixture.path).unwrap();
        let root = fixture.folder.canonicalize().unwrap();
        let first = seed_video(&connection, &fixture, "first.mp4");
        let second = seed_video(&connection, &fixture, "second.mp4");
        std::fs::write(&second.path, b"video replaced by different data").unwrap();
        let pair = [first.clone(), second.clone()];
        assert!(assignment_on(&mut connection, &root, &pair, Assignment::Create { name: "Not created".into() }).is_err());
        assert!(list_on(&connection, "").unwrap().is_empty());
        create_on(&connection, "Keeper").unwrap();
        let id = list_on(&connection, "").unwrap()[0].id;
        assert!(assignment_on(&mut connection, &root, &pair, Assignment::Set { tag_id: id, assigned: true }).is_err());
        assert_eq!(list_on(&connection, "").unwrap()[0].video_count, 0);
        assert!(assignment_on(&mut connection, &root.join("unrelated"), std::slice::from_ref(&first), Assignment::Set { tag_id: id, assigned: true }).is_err());
        assert!(assignment_on(&mut connection, &root, &[], Assignment::Set { tag_id: id, assigned: true }).is_err());
        assert!(assignment_on(&mut connection, &root, std::slice::from_ref(&first), Assignment::Set { tag_id: 999, assigned: true }).is_err());
        let wrong_id = TagVideo { video_id: 999, path: first.path.clone() };
        assert!(assignment_on(&mut connection, &root, &[wrong_id], Assignment::Set { tag_id: id, assigned: true }).is_err());
        let folder = TagVideo { video_id: first.video_id, path: root.to_string_lossy().into_owned() };
        assert!(assignment_on(&mut connection, &root, &[folder], Assignment::Set { tag_id: id, assigned: true }).is_err());
        std::fs::remove_file(&second.path).unwrap();
        assert!(assignment_on(&mut connection, &root, &[first, second], Assignment::Set { tag_id: id, assigned: true }).is_err());
        assert_eq!(list_on(&connection, "").unwrap()[0].video_count, 0);
    }
}
