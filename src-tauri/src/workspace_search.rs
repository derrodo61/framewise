use std::{fs, path::{Path, PathBuf}, sync::{Arc, Mutex, OnceLock, atomic::{AtomicBool, AtomicU64, Ordering}}};
use rusqlite::{Connection, params};
use serde::Serialize;

const MAX_ITEMS: usize = 100_000;
const MAX_FOLDER_ITEMS: usize = 10_000;
const PAGE_SIZE: usize = 200;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ScanStatus {
    id: u64, root: String, status: String, folders: usize, videos: usize,
    warnings: usize, message: Option<String>, current_folder: String,
}
struct Job { status: Mutex<ScanStatus>, cancelled: AtomicBool }
#[derive(Default)]
pub(crate) struct ScanState(Mutex<Option<Arc<Job>>>);
fn serial_scan() -> &'static Mutex<()> { static LOCK: OnceLock<Mutex<()>> = OnceLock::new(); LOCK.get_or_init(|| Mutex::new(())) }

fn bounds(root: &Path) -> (String, String) {
    let text = root.to_string_lossy();
    let prefix = format!("{}{}", text.trim_end_matches(std::path::MAIN_SEPARATOR), std::path::MAIN_SEPARATOR);
    let upper = format!("{prefix}\u{10ffff}");
    (prefix, upper)
}

struct Folder { present: Vec<String>, observed: Vec<crate::catalog::ObservedVideo>, children: Vec<PathBuf>, entries: usize }
fn read_folder(path: &Path, root: &Path, cancelled: &AtomicBool) -> Result<Folder, String> {
    if cancelled.load(Ordering::Relaxed) { return Err("Scan cancelled.".into()); }
    let canonical = path.canonicalize().map_err(|error| error.to_string())?;
    if canonical != path || !canonical.starts_with(root) { return Err("Folder changed or is a link; skipped.".into()); }
    let mut folder = Folder { present: Vec::new(), observed: Vec::new(), children: Vec::new(), entries: 0 };
    for (index, item) in fs::read_dir(path).map_err(|error| error.to_string())?.enumerate() {
        if cancelled.load(Ordering::Relaxed) { return Err("Scan cancelled.".into()); }
        if index >= MAX_FOLDER_ITEMS { return Err("Folder exceeds the 10,000 entry scan limit; skipped.".into()); }
        folder.entries += 1;
        let item = item.map_err(|error| error.to_string())?;
        let kind = item.file_type().map_err(|error| error.to_string())?;
        if kind.is_symlink() { continue; }
        let child = item.path();
        if kind.is_dir() { folder.children.push(child); }
        else if kind.is_file() && crate::video_file(&child) {
            folder.present.push(child.to_string_lossy().into_owned());
            // Preserve unreadable-but-present records instead of declaring them missing.
            folder.observed.push(crate::catalog::observe(&child)?);
        }
    }
    Ok(folder)
}

fn scan_on(connection: &mut Connection, root: &Path, job: &Job) -> Result<String, String> {
    scan_with_progress(connection, root, job, || {})
}
fn scan_with_progress(connection: &mut Connection, root: &Path, job: &Job, mut after_folder: impl FnMut()) -> Result<String, String> {
    connection.execute("UPDATE workspace_scans SET status='failed',completed_at=unixepoch()*1000 WHERE status='running'", []).map_err(|error| error.to_string())?;
    connection.execute("INSERT INTO workspace_scans(workspace_path,started_at,status) VALUES (?1,unixepoch()*1000,'running')", [root.to_string_lossy().as_ref()]).map_err(|error| error.to_string())?;
    let scan_id = connection.last_insert_rowid();
    let result = (|| {
        let mut pending = vec![root.to_path_buf()];
        let mut complete = Vec::new();
        let mut count = 0;
        let mut warnings = 0;
        while let Some(folder) = pending.pop() {
            if job.cancelled.load(Ordering::Relaxed) { return Ok("cancelled".to_string()); }
            job.status.lock().map_err(|_| "Scan state unavailable")?.current_folder = folder.to_string_lossy().into_owned();
            match read_folder(&folder, root, &job.cancelled) {
                Ok(found) => {
                    count += found.entries + 1;
                    if count > MAX_ITEMS { return Err("Workspace exceeds the 100,000 item scan limit. Choose a smaller workspace.".into()); }
                    if job.cancelled.load(Ordering::Relaxed) { return Ok("cancelled".to_string()); }
                    crate::catalog::register_folder_on(connection, &folder, &found.present, &found.observed)?;
                    let mut status = job.status.lock().map_err(|_| "Scan state unavailable")?;
                    status.folders += 1; status.videos += found.observed.len();
                    complete.push(folder);
                    pending.extend(found.children);
                    after_folder();
                }
                Err(message) => {
                    if job.cancelled.load(Ordering::Relaxed) { return Ok("cancelled".to_string()); }
                    warnings += 1;
                    let mut status = job.status.lock().map_err(|_| "Scan state unavailable")?;
                    status.warnings = warnings;
                    status.message = Some(format!("Skipped {}: {message}", folder.display()));
                }
            }
        }
        if job.cancelled.load(Ordering::Relaxed) { return Ok("cancelled".to_string()); }
        // Only a completely successful traversal can retire records in vanished subfolders.
        if warnings == 0 {
            let transaction = connection.transaction().map_err(|error| error.to_string())?;
            transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS scanned_folders(path TEXT PRIMARY KEY); DELETE FROM scanned_folders;").map_err(|error| error.to_string())?;
            for folder in complete { transaction.execute("INSERT INTO scanned_folders VALUES (?1)", [folder.to_string_lossy().as_ref()]).map_err(|error| error.to_string())?; }
            let (lower, upper) = bounds(root);
            transaction.execute("UPDATE videos SET status='missing' WHERE status='active' AND path>=?1 AND path<?2 AND parent_path NOT IN (SELECT path FROM scanned_folders)", params![lower, upper]).map_err(|error| error.to_string())?;
            transaction.commit().map_err(|error| error.to_string())?;
        }
        Ok("complete".to_string())
    })();
    let status = result.as_deref().unwrap_or("failed");
    connection.execute("UPDATE workspace_scans SET status=?1,completed_at=unixepoch()*1000 WHERE id=?2", params![status, scan_id]).map_err(|error| error.to_string())?;
    result
}

#[tauri::command]
pub(crate) fn start_workspace_scan(expected_root: String, state: tauri::State<'_, crate::AppState>, scans: tauri::State<'_, ScanState>) -> Result<ScanStatus, String> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let root = crate::selected_root(&state)?;
    if root.to_string_lossy() != expected_root { return Err("Workspace changed; scan discarded.".into()); }
    let mut slot = scans.0.lock().map_err(|_| "Scan state unavailable")?;
    if let Some(previous) = slot.as_ref() {
        previous.cancelled.store(true, Ordering::Relaxed);
    }
    let status = ScanStatus { id: NEXT.fetch_add(1, Ordering::Relaxed), root: root.to_string_lossy().into_owned(), status: "running".into(), folders: 0, videos: 0, warnings: 0, message: None, current_folder: root.to_string_lossy().into_owned() };
    let job = Arc::new(Job { status: Mutex::new(status.clone()), cancelled: AtomicBool::new(false) });
    *slot = Some(job.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let _serial = serial_scan().lock().map_err(|_| "Scan worker unavailable")?;
            if job.cancelled.load(Ordering::Relaxed) { return Ok("cancelled".into()); }
            crate::catalog_operations::recover_pending()?;
            let _lease = crate::catalog_operations::reserve_discovery(&root)?;
            let mut connection = crate::database::open_at(&crate::database::database_path()?)?;
            scan_on(&mut connection, &root, &job)
        })();
        if let Ok(mut status) = job.status.lock() {
            match result { Ok(value) => status.status = value, Err(error) => { status.status = "failed".into(); status.message = Some(error); } }
        }
    });
    Ok(status)
}

#[tauri::command]
pub(crate) fn workspace_scan_status(scan_id: u64, scans: tauri::State<'_, ScanState>) -> Result<ScanStatus, String> {
    let slot = scans.0.lock().map_err(|_| "Scan state unavailable")?;
    let status = slot.as_ref().ok_or("No scan is running")?.status.lock().map_err(|_| "Scan state unavailable")?;
    if status.id != scan_id { return Err("This scan was replaced.".into()); }
    Ok(status.clone())
}
#[tauri::command]
pub(crate) fn cancel_workspace_scan(scan_id: u64, scans: tauri::State<'_, ScanState>) -> Result<(), String> {
    let slot = scans.0.lock().map_err(|_| "Scan state unavailable")?;
    if let Some(job) = slot.as_ref() && job.status.lock().map_err(|_| "Scan state unavailable")?.id == scan_id { job.cancelled.store(true, Ordering::Relaxed); }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SearchPage { entries: Vec<crate::FileEntry>, total: usize, total_videos: usize, page: usize }
fn search_on(connection: &mut Connection, root: &Path, tags: &[i64], match_all: bool, sort: &str, descending: bool, page: usize) -> Result<SearchPage, String> {
    let transaction = connection.transaction().map_err(|error| error.to_string())?;
    transaction.execute_batch("CREATE TEMP TABLE IF NOT EXISTS workspace_filter_tags(id INTEGER PRIMARY KEY); DELETE FROM workspace_filter_tags;").map_err(|error| error.to_string())?;
    for tag in tags { transaction.execute("INSERT OR IGNORE INTO workspace_filter_tags VALUES (?1)", [tag]).map_err(|error| error.to_string())?; }
    let count: i64 = transaction.query_row("SELECT COUNT(*) FROM workspace_filter_tags", [], |row| row.get::<_, i64>(0)).map_err(|error| error.to_string())?;
    let (lower, upper) = bounds(root);
    let predicate = "v.status='active' AND v.path>=?1 AND v.path<?2 AND (?3=0 OR (?4 AND (SELECT COUNT(*) FROM video_tags vt JOIN workspace_filter_tags f ON f.id=vt.tag_id WHERE vt.video_id=v.id)=?3) OR (NOT ?4 AND EXISTS(SELECT 1 FROM video_tags vt JOIN workspace_filter_tags f ON f.id=vt.tag_id WHERE vt.video_id=v.id)))";
    let total = transaction.query_row(&format!("SELECT COUNT(*) FROM videos v WHERE {predicate}"), params![lower, upper, count, match_all], |row| row.get::<_, i64>(0)).map_err(|error| error.to_string())?;
    let total_videos = transaction.query_row("SELECT COUNT(*) FROM videos WHERE status='active' AND path>=?1 AND path<?2", params![lower, upper], |row| row.get::<_, i64>(0)).map_err(|error| error.to_string())?;
    let total = usize::try_from(total).map_err(|error| error.to_string())?;
    let total_videos = usize::try_from(total_videos).map_err(|error| error.to_string())?;
    let page = page.min(total.saturating_sub(1) / PAGE_SIZE);
    let direction = if descending { "DESC" } else { "ASC" };
    let order = if sort == "modified" { "CAST(v.modified_ns AS INTEGER)" } else { "v.name COLLATE NOCASE" };
    let entries = {
        let mut statement = transaction.prepare(&format!("SELECT v.id,v.path,v.name,v.size,v.modified_ns FROM videos v WHERE {predicate} ORDER BY {order} {direction},v.name COLLATE NOCASE {direction},v.path {direction} LIMIT ?5 OFFSET ?6")).map_err(|error| error.to_string())?;
        statement.query_map(params![lower, upper, count, match_all, PAGE_SIZE as i64, (page * PAGE_SIZE) as i64], |row| {
            let modified: String = row.get(4)?;
            Ok(crate::FileEntry { video_id: Some(row.get(0)?), path: row.get(1)?, name: row.get(2)?, size: u64::try_from(row.get::<_, i64>(3)?).ok(), modified_at: modified.parse::<u128>().ok().and_then(|n| u64::try_from(n / 1_000_000).ok()), modified_ns: Some(modified), is_directory: false })
        }).map_err(|error| error.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
    };
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(SearchPage { entries, total, total_videos, page })
}
#[tauri::command]
pub(crate) async fn search_workspace(expected_root: String, tag_ids: Vec<i64>, match_all: bool, sort: String, descending: bool, page: usize, state: tauri::State<'_, crate::AppState>) -> Result<SearchPage, String> {
    let root = crate::selected_root(&state)?;
    if root.to_string_lossy() != expected_root { return Err("Workspace changed; results discarded.".into()); }
    tauri::async_runtime::spawn_blocking(move || crate::database::with_connection(|connection| search_on(connection, &root, &tag_ids, match_all, &sort, descending, page))).await.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{open_at, tests::Fixture};
    fn job(root: &Path) -> Job {
        Job { cancelled: AtomicBool::new(false), status: Mutex::new(ScanStatus { id: 1, root: root.to_string_lossy().into_owned(), status: "running".into(), folders: 0, videos: 0, warnings: 0, message: None, current_folder: String::new() }) }
    }
    #[test]
    fn recursive_discovery_preserves_identity_and_retires_removed_subtrees() {
        let fixture = Fixture::new();
        let root = fixture.folder.canonicalize().unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("clip.mp4"), b"first").unwrap();
        fs::write(root.join("nested/clip.mp4"), b"second").unwrap();
        fs::write(root.join("ignored.txt"), b"not video").unwrap();
        let mut connection = open_at(&fixture.path).unwrap();
        assert_eq!(scan_on(&mut connection, &root, &job(&root)).unwrap(), "complete");
        let first = search_on(&mut connection, &root, &[], true, "name", false, 0).unwrap();
        assert_eq!(first.total, 2);
        assert_ne!(first.entries[0].path, first.entries[1].path);
        let ids = first.entries.iter().map(|entry| entry.video_id).collect::<Vec<_>>();
        scan_on(&mut connection, &root, &job(&root)).unwrap();
        assert_eq!(search_on(&mut connection, &root, &[], true, "name", false, 0).unwrap().entries.iter().map(|entry| entry.video_id).collect::<Vec<_>>(), ids);
        // Delete only this test's explicitly known temporary subtree.
        fs::remove_file(root.join("nested/clip.mp4")).unwrap();
        fs::remove_dir(root.join("nested")).unwrap();
        scan_on(&mut connection, &root, &job(&root)).unwrap();
        assert_eq!(search_on(&mut connection, &root, &[], true, "name", false, 0).unwrap().total, 1);
        fs::remove_file(root.join("clip.mp4")).unwrap();
        scan_on(&mut connection, &root, &job(&root)).unwrap();
        assert_eq!(search_on(&mut connection, &root, &[], true, "name", false, 0).unwrap().total, 0);
    }
    #[test]
    fn cancelled_and_unreadable_scans_preserve_known_videos() {
        let fixture = Fixture::new();
        let fixture_root = fixture.folder.canonicalize().unwrap();
        let root = fixture_root.join("media");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("clip.mp4"), b"clip").unwrap();
        let mut connection = open_at(&fixture.path).unwrap();
        scan_on(&mut connection, &root, &job(&root)).unwrap();
        fs::remove_file(root.join("clip.mp4")).unwrap();
        let cancelled = job(&root);
        cancelled.cancelled.store(true, Ordering::Relaxed);
        assert_eq!(scan_on(&mut connection, &root, &cancelled).unwrap(), "cancelled");
        assert_eq!(search_on(&mut connection, &root, &[], true, "name", false, 0).unwrap().total, 1);
        // A directory read failure models an unavailable root/drive without platform ACL assumptions.
        let moved = root.with_file_name(format!("{}-unavailable", root.file_name().unwrap().to_string_lossy()));
        assert!(root.starts_with(&fixture_root) && moved.starts_with(&fixture_root));
        fs::rename(&root, &moved).unwrap();
        let failed = job(&root);
        let outcome = scan_on(&mut connection, &root, &failed);
        fs::rename(&moved, &root).unwrap();
        assert_eq!(outcome.unwrap(), "complete");
        assert_eq!(failed.status.lock().unwrap().warnings, 1);
        assert_eq!(search_on(&mut connection, &root, &[], true, "name", false, 0).unwrap().total, 1);
    }
    #[test]
    fn indexed_workspace_queries_filter_sort_page_and_respect_path_boundaries() {
        let fixture = Fixture::new();
        let root = fixture.folder.canonicalize().unwrap();
        let mut connection = open_at(&fixture.path).unwrap();
        connection.execute("INSERT INTO tags(name,normalized_name) VALUES ('One','one'),('Two','two')", []).unwrap();
        for index in 0..450 {
            let parent = root.join(if index % 2 == 0 { "a" } else { "b" });
            connection.execute("INSERT INTO videos(path,parent_path,name,size,modified_ns,status,last_seen_at) VALUES (?1,?2,?3,1,?4,'active',1)", params![parent.join(format!("{index:03}.mp4")).to_string_lossy(), parent.to_string_lossy(), format!("{index:03}.mp4"), index.to_string()]).unwrap();
            let id = connection.last_insert_rowid();
            connection.execute("INSERT INTO video_tags VALUES (?1,1)", [id]).unwrap();
            if index % 2 == 0 { connection.execute("INSERT INTO video_tags VALUES (?1,2)", [id]).unwrap(); }
        }
        let sibling = PathBuf::from(format!("{}-other", root.display()));
        connection.execute("INSERT INTO videos(path,parent_path,name,size,modified_ns,status,last_seen_at) VALUES (?1,?2,'other.mp4',1,'0','active',1)", params![sibling.join("other.mp4").to_string_lossy(), sibling.to_string_lossy()]).unwrap();
        let all = search_on(&mut connection, &root, &[1, 2, 2], true, "name", false, 0).unwrap();
        assert_eq!((all.total, all.total_videos, all.entries.len()), (225, 450, PAGE_SIZE));
        let any = search_on(&mut connection, &root, &[1, 2], false, "name", false, 1).unwrap();
        assert_eq!(any.total, 450);
        assert_eq!(any.entries[0].name, "200.mp4");
        let last = search_on(&mut connection, &root, &[], true, "modified", true, usize::MAX).unwrap();
        assert_eq!(last.page, 2);
        assert_eq!(last.entries.len(), 50);
        assert_eq!(last.entries[0].name, "049.mp4");
        assert_eq!(search_on(&mut connection, &root, &[999], true, "name", false, 0).unwrap().total, 0);
        connection.execute("UPDATE tags SET name='Renamed',normalized_name='renamed' WHERE id=2", []).unwrap();
        assert_eq!(search_on(&mut connection, &root, &[2], true, "name", false, 0).unwrap().total, 225);
        connection.execute("DELETE FROM tags WHERE id=2", []).unwrap();
        assert_eq!(search_on(&mut connection, &root, &[2], true, "name", false, 0).unwrap().total, 0);
    }
    #[test]
    fn folder_reader_obeys_cancellation_and_skips_links() {
        let fixture = Fixture::new();
        let root = fixture.folder.canonicalize().unwrap();
        let cancelled = AtomicBool::new(true);
        assert!(read_folder(&root, &root, &cancelled).is_err());
        assert!(read_folder(&fixture.path, &root, &AtomicBool::new(false)).is_err());
        #[cfg(unix)] {
            std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
            assert!(read_folder(&root, &root, &AtomicBool::new(false)).unwrap().children.is_empty());
        }
    }
    #[test]
    fn discovery_lease_blocks_overlapping_changes_until_released() {
        let fixture = Fixture::new();
        let root = fixture.folder.canonicalize().unwrap();
        let lease = crate::catalog_operations::reserve_discovery(&root).unwrap();
        assert!(crate::catalog_operations::ensure_paths_idle(&[root.join("clip.mp4")]).is_err());
        assert!(crate::catalog_operations::reserve_discovery(&root).is_err());
        drop(lease);
        assert!(crate::catalog_operations::ensure_paths_idle(&[root.join("clip.mp4")]).is_ok());
    }
    #[test]
    fn interrupted_scan_does_not_retire_an_unvisited_subtree() {
        let fixture = Fixture::new();
        let root = fixture.folder.canonicalize().unwrap();
        fs::create_dir(root.join("nested")).unwrap();
        fs::write(root.join("nested/clip.mp4"), b"clip").unwrap();
        let mut connection = open_at(&fixture.path).unwrap();
        scan_on(&mut connection, &root, &job(&root)).unwrap();
        fs::remove_file(root.join("nested/clip.mp4")).unwrap();
        fs::remove_dir(root.join("nested")).unwrap();
        let interrupted = job(&root);
        assert_eq!(scan_with_progress(&mut connection, &root, &interrupted, || interrupted.cancelled.store(true, Ordering::Relaxed)).unwrap(), "cancelled");
        assert_eq!(search_on(&mut connection, &root, &[], true, "name", false, 0).unwrap().total, 1);
    }
}
