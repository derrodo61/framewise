use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use crate::database::{normalize_tag_name, tag_search_key, with_connection};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Tag { id: i64, name: String, video_count: i64 }

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
}
