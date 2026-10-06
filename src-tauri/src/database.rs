use std::{fs, path::{Path, PathBuf}, sync::{Mutex, OnceLock}, time::{Duration, SystemTime, UNIX_EPOCH}};
use caseless::Caseless;
use rusqlite::{Connection, functions::FunctionFlags};
use unicode_normalization::UnicodeNormalization;

pub(crate) fn database_path() -> Result<PathBuf, String> {
    Ok(crate::preferences::settings_dir()?.join("framewise.db"))
}

pub(crate) fn normalize_tag_name(name: &str) -> Result<(String, String), String> {
    let display = name.trim();
    if display.is_empty() || display.chars().count() > 100 || display.chars().any(char::is_control) {
        return Err("Choose a tag name with 1–100 characters and no control characters.".into());
    }
    let key = tag_search_key(display);
    Ok((display.to_owned(), key))
}
pub(crate) fn tag_search_key(query: &str) -> String { query.trim().nfkc().default_case_fold().nfkc().collect() }

pub(crate) fn open_at(path: &Path) -> Result<Connection, String> {
    fs::create_dir_all(path.parent().ok_or("Cannot locate Framewise database folder")?)
        .map_err(|error| format!("Cannot create Framewise database folder: {error}"))?;
    let mut connection = Connection::open(path).map_err(|error| format!("Cannot open Framewise database: {error}"))?;
    connection.busy_timeout(Duration::from_secs(3)).map_err(|error| error.to_string())?;
    connection.pragma_update(None, "foreign_keys", true).map_err(|error| error.to_string())?;
    connection.create_scalar_function("framewise_tag_key", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |context| {
        let input = context.get::<String>(0)?;
        normalize_tag_name(&input).and_then(|(display, key)| {
            if display != input { Err("Tag names must not have surrounding whitespace.".into()) }
            else { Ok(key) }
        })
            .map_err(|error| rusqlite::Error::UserFunctionError(Box::new(std::io::Error::new(std::io::ErrorKind::InvalidInput, error))))
    }).map_err(|error| error.to_string())?;
    let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(|error| error.to_string())?;
    if version != 0 && version != 1 { return Err(format!("Unsupported Framewise database version: {version}")); }
    connection.pragma_update(None, "journal_mode", "WAL").map_err(|error| error.to_string())?;
    if version == 0 {
        connection.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS previews (video_path TEXT PRIMARY KEY NOT NULL, version TEXT NOT NULL, image_name TEXT NOT NULL);
            PRAGMA user_version=1;
            COMMIT;").map_err(|error| format!("Cannot initialize preview database: {error}"))?;
    }
    migrate_catalog(&mut connection, path, version == 1)?;
    Ok(connection)
}

fn catalog_version(connection: &Connection) -> Result<i64, String> {
    let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_schema')", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if !exists { return Ok(0); }
    connection.query_row("SELECT version FROM catalog_schema WHERE id=1", [], |row| row.get(0))
        .map_err(|error| format!("Cannot read catalog schema version: {error}"))
}

fn migrate_catalog(connection: &mut Connection, path: &Path, backup_existing: bool) -> Result<(), String> {
    match catalog_version(connection)? {
        3 => return Ok(()),
        1 | 2 => {},
        0 => {},
        version => return Err(format!("Unsupported Framewise catalog version: {version}")),
    }
    if backup_existing {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos();
        let backup = path.with_file_name(format!("framewise-before-catalog-v3-{stamp}.db"));
        // SQLite makes a consistent snapshot including committed WAL contents.
        connection.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])
            .map_err(|error| format!("Cannot back up database before migration: {error}"))?;
        fs::OpenOptions::new().read(true).write(true).open(&backup).and_then(|file| file.sync_all())
            .map_err(|error| format!("Cannot flush database backup: {error}"))?;
        log::info!("Database backup before catalog migration: {}", backup.display());
    }
    let transaction = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate).map_err(|error| error.to_string())?;
    // Another Framewise process might have completed the migration while we backed up.
    let version = catalog_version(&transaction)?;
    if !(0..=3).contains(&version) { return Err(format!("Unsupported Framewise catalog version: {version}")); }
    if version == 0 {
        transaction.execute_batch("CREATE TABLE catalog_schema (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL);
            INSERT INTO catalog_schema VALUES (1, 1);
            CREATE TABLE videos (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL,
                parent_path TEXT NOT NULL,
                name TEXT NOT NULL,
                size INTEGER NOT NULL CHECK(size >= 0),
                modified_ns TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('active','missing','changed','trashed')),
                last_seen_at INTEGER NOT NULL
            );
            CREATE UNIQUE INDEX videos_active_path ON videos(path) WHERE status='active';
            CREATE INDEX videos_parent_status ON videos(parent_path, status);
            CREATE INDEX videos_history_path ON videos(path);
            CREATE TABLE tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL CHECK(name=trim(name) AND length(name) BETWEEN 1 AND 100),
                normalized_name TEXT NOT NULL UNIQUE CHECK(normalized_name=framewise_tag_key(name))
            );
            CREATE TABLE video_tags (
                video_id INTEGER NOT NULL REFERENCES videos(id) ON DELETE CASCADE,
                tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (video_id, tag_id)
            );
            CREATE INDEX video_tags_by_tag ON video_tags(tag_id, video_id);
            CREATE TABLE folder_discovery (path TEXT PRIMARY KEY, last_completed_at INTEGER NOT NULL);
            CREATE TABLE workspace_scans (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_path TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                completed_at INTEGER,
                status TEXT NOT NULL CHECK(status IN ('running','complete','cancelled','failed'))
            );
            CREATE INDEX workspace_scans_by_workspace ON workspace_scans(workspace_path, id);")
            .map_err(|error| format!("Cannot migrate video catalog: {error}"))?;
    }
    if version < 2 {
        transaction.execute_batch("CREATE TABLE catalog_operation_receipts (operation_id TEXT PRIMARY KEY, completed_at INTEGER NOT NULL);
            UPDATE catalog_schema SET version=2 WHERE id=1;").map_err(|error| format!("Cannot migrate catalog operations: {error}"))?;
    }
    if version < 3 {
        transaction.execute_batch("ALTER TABLE videos ADD COLUMN content_hash TEXT;
            UPDATE catalog_schema SET version=3 WHERE id=1;").map_err(|error| format!("Cannot migrate restored-video identity: {error}"))?;
    }
    transaction.commit().map_err(|error| format!("Cannot commit catalog migration: {error}"))
}

pub(crate) fn with_connection<T>(operation: impl FnOnce(&mut Connection) -> Result<T, String>) -> Result<T, String> {
    static DATABASE: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();
    let mut guard = DATABASE.get_or_init(|| Mutex::new(None)).lock().map_err(|_| "Framewise database is unavailable".to_string())?;
    if guard.is_none() { *guard = Some(open_at(&database_path()?)?); }
    operation(guard.as_mut().ok_or("Framewise database is unavailable")?)
}

pub(crate) fn initialize() -> Result<(), String> { with_connection(|_| Ok(())) }

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) struct Fixture { pub folder: PathBuf, pub path: PathBuf }
    impl Fixture {
        pub fn new() -> Self {
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let folder = std::env::temp_dir().join(format!("framewise-catalog-{}-{stamp}", std::process::id()));
            fs::create_dir(&folder).unwrap();
            let path = folder.join("framewise.db");
            Self { folder, path }
        }
        fn backups(&self) -> Vec<PathBuf> {
            fs::read_dir(&self.folder).unwrap().map(|entry| entry.unwrap().path())
                .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("framewise-before-catalog-")).collect()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { fs::remove_dir_all(&self.folder).unwrap(); }
    }

    #[test]
    fn migration_snapshots_wal_and_keeps_legacy_preview_access() {
        let fixture = Fixture::new();
        let legacy = Connection::open(&fixture.path).unwrap();
        legacy.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE previews(video_path TEXT PRIMARY KEY,version TEXT NOT NULL,image_name TEXT NOT NULL);
            PRAGMA user_version=1;
            INSERT INTO previews VALUES ('clip.mp4','version-a','cached.png');").unwrap();
        // Keep the legacy connection open: the snapshot must include committed WAL data.
        let connection = open_at(&fixture.path).unwrap();
        assert_eq!(connection.query_row("SELECT image_name FROM previews WHERE video_path='clip.mp4'", [], |row| row.get::<_, String>(0)).unwrap(), "cached.png");
        assert_eq!(catalog_version(&connection).unwrap(), 3);
        let backups = fixture.backups();
        assert_eq!(backups.len(), 1);
        let backup = Connection::open(&backups[0]).unwrap();
        assert_eq!(backup.query_row("SELECT image_name FROM previews", [], |row| row.get::<_, String>(0)).unwrap(), "cached.png");
        assert_eq!(catalog_version(&backup).unwrap(), 0);
        assert_eq!(legacy.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), 1);
        // Simulate the installed app, which does not register the tag SQL function.
        legacy.execute("INSERT INTO previews VALUES ('other.mp4','version-b','other.png')", []).unwrap();
        legacy.execute("UPDATE previews SET version='version-c' WHERE video_path='clip.mp4'", []).unwrap();
        drop(backup); drop(connection);
        let reopened = open_at(&fixture.path).unwrap();
        assert_eq!(reopened.query_row("SELECT COUNT(*) FROM previews", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(fixture.backups().len(), 1);
    }

    #[test]
    fn failed_migration_rolls_back_all_catalog_tables() {
        let fixture = Fixture::new();
        let connection = Connection::open(&fixture.path).unwrap();
        connection.execute_batch("CREATE TABLE previews(video_path TEXT PRIMARY KEY, version TEXT, image_name TEXT);
            PRAGMA user_version=1; CREATE TABLE tags(original_data TEXT); INSERT INTO tags VALUES ('keep me');").unwrap();
        drop(connection);
        assert!(open_at(&fixture.path).err().unwrap().contains("Cannot migrate video catalog"));
        let unchanged = Connection::open(&fixture.path).unwrap();
        assert_eq!(catalog_version(&unchanged).unwrap(), 0);
        assert_eq!(unchanged.query_row("SELECT original_data FROM tags", [], |row| row.get::<_, String>(0)).unwrap(), "keep me");
        assert_eq!(unchanged.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name='videos'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(fixture.backups().len(), 1);
    }

    #[test]
    fn tag_names_are_unicode_normalized_and_database_enforced() {
        let fixture = Fixture::new();
        let connection = open_at(&fixture.path).unwrap();
        assert_eq!(normalize_tag_name("  Straße  ").unwrap(), ("Straße".into(), "strasse".into()));
        assert_eq!(normalize_tag_name("STRASSE").unwrap().1, "strasse");
        assert_eq!(normalize_tag_name("Café").unwrap().1, normalize_tag_name("Cafe\u{301}").unwrap().1);
        assert!(normalize_tag_name(" \t ").is_err());
        assert!(normalize_tag_name("a\nb").is_err());
        assert!(normalize_tag_name(&"x".repeat(101)).is_err());
        connection.execute("INSERT INTO tags(name,normalized_name) VALUES (?1,?2)", ["Straße", "strasse"]).unwrap();
        assert!(connection.execute("INSERT INTO tags(name,normalized_name) VALUES (?1,?2)", ["STRASSE", "strasse"]).is_err());
        assert!(connection.execute("INSERT INTO tags(name,normalized_name) VALUES ('Other','incorrect-key')", []).is_err());
        assert!(connection.execute("INSERT INTO tags(name,normalized_name) VALUES ('','')", []).is_err());
        assert!(connection.execute("INSERT INTO tags(name,normalized_name) VALUES (?1,'other')", ["\u{a0}Other\u{a0}"]).is_err());
        assert!(connection.execute("INSERT INTO video_tags(video_id,tag_id) VALUES (999,1)", []).is_err());
        assert_eq!(connection.pragma_query_value(None, "foreign_keys", |row| row.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn phase_one_catalog_upgrade_retains_ids_tags_and_previews() {
        let fixture = Fixture::new();
        let connection = open_at(&fixture.path).unwrap();
        connection.execute_batch("INSERT INTO previews VALUES ('clip.mp4','v1','cached.png');
            INSERT INTO videos(id,path,parent_path,name,size,modified_ns,status,last_seen_at) VALUES (7,'clip.mp4','.','clip.mp4',100,'123','active',1);
            INSERT INTO tags(id,name,normalized_name) VALUES (5,'Keeper','keeper');
            INSERT INTO video_tags VALUES (7,5);
            DROP TABLE catalog_operation_receipts;
            ALTER TABLE videos DROP COLUMN content_hash;
            UPDATE catalog_schema SET version=1;").unwrap();
        drop(connection);
        let upgraded = open_at(&fixture.path).unwrap();
        assert_eq!(catalog_version(&upgraded).unwrap(), 3);
        assert_eq!(upgraded.query_row("SELECT video_id FROM video_tags WHERE tag_id=5", [], |row| row.get::<_, i64>(0)).unwrap(), 7);
        assert_eq!(upgraded.query_row("SELECT image_name FROM previews", [], |row| row.get::<_, String>(0)).unwrap(), "cached.png");
        assert_eq!(fixture.backups().len(), 1);
        let backup = Connection::open(&fixture.backups()[0]).unwrap();
        assert_eq!(catalog_version(&backup).unwrap(), 1);
        assert_eq!(backup.query_row("SELECT video_id FROM video_tags", [], |row| row.get::<_, i64>(0)).unwrap(), 7);
    }
}
