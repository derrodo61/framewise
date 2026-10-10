use std::{collections::HashMap, fs, io::Write, path::{Path, PathBuf}, sync::{Mutex, OnceLock}, time::{SystemTime, UNIX_EPOCH}};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use crate::catalog::{ObservedVideo, ensure_observed_on, observe};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) enum Action { Move, Copy, Replace, Trash }
#[derive(Clone, Serialize, Deserialize)]
struct Item { id: i64, root: usize, before: ObservedVideo, relative: PathBuf, #[serde(default)] content_hash: Option<String> }
#[derive(Clone, Serialize, Deserialize)]
struct Plan { version: u32, id: String, action: Action, sources: Vec<PathBuf>, targets: Vec<Option<PathBuf>>, targets_existed: Vec<bool>, items: Vec<Item> }
#[derive(Serialize, Deserialize)]
struct FinishedItem { item: Item, after: Option<ObservedVideo> }
#[derive(Serialize, Deserialize)]
struct Completion { version: u32, id: String, action: Action, items: Vec<FinishedItem> }
pub(crate) struct Operation { directory: PathBuf, plan: Plan }

fn live() -> &'static Mutex<HashMap<String, Vec<PathBuf>>> { static LIVE: OnceLock<Mutex<HashMap<String, Vec<PathBuf>>>> = OnceLock::new(); LIVE.get_or_init(|| Mutex::new(HashMap::new())) }
pub(crate) struct DiscoveryLease(String);
impl Drop for DiscoveryLease {
    fn drop(&mut self) { if let Ok(mut operations) = live().lock() { operations.remove(&self.0); } }
}
pub(crate) fn reserve_discovery(root: &Path) -> Result<DiscoveryLease, String> {
    let mut operations = live().lock().map_err(|_| "Catalog operation state unavailable")?;
    if operations.values().flatten().any(|path| path.starts_with(root) || root.starts_with(path)) {
        return Err("A file operation is using this workspace. Try scanning again when it finishes.".into());
    }
    let id = format!("discovery-{}", root.display());
    operations.insert(id.clone(), vec![root.to_path_buf()]);
    Ok(DiscoveryLease(id))
}
fn journal_directory() -> Result<PathBuf, String> { Ok(crate::preferences::settings_dir()?.join("catalog-operations")) }
pub(crate) fn ensure_paths_idle(paths: &[PathBuf]) -> Result<(), String> {
    if live().lock().map_err(|_| "Catalog operation state unavailable")?.values().flatten().any(|busy| paths.iter().any(|path| busy.starts_with(path) || path.starts_with(busy))) {
        return Err("A file operation is using one of these videos. Wait for it to finish.".into());
    }
    Ok(())
}
fn journal_path(directory: &Path, id: &str, stage: &str) -> PathBuf { directory.join(format!("{id}.{stage}.json")) }
fn write_journal<T: Serialize>(directory: &Path, id: &str, stage: &str, value: &T) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let target = journal_path(directory, id, stage);
    let temporary = target.with_extension("tmp");
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|error| error.to_string())?;
    output.write_all(&bytes).and_then(|_| output.sync_all()).map_err(|error| error.to_string())?;
    drop(output);
    fs::rename(&temporary, &target).map_err(|error| error.to_string())
}
fn remove_journals(directory: &Path, id: &str) -> Result<(), String> {
    for stage in ["completed", "prepared"] {
        let path = journal_path(directory, id, stage);
        match fs::remove_file(path) { Ok(()) => {}, Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}, Err(error) => return Err(error.to_string()) }
    }
    Ok(())
}

fn collect(path: &Path, root: usize, base: &Path, videos: &mut Vec<(usize, PathBuf, ObservedVideo)>) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() { return Ok(()); }
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            collect(&entry.map_err(|error| error.to_string())?.path(), root, base, videos)?;
        }
    } else if metadata.is_file() && crate::media_file(path) {
        videos.push((root, path.strip_prefix(base).map_err(|error| error.to_string())?.to_path_buf(), observe(path)?));
    }
    Ok(())
}

impl Operation {
    pub(crate) fn begin(action: Action, sources: Vec<PathBuf>, targets: Vec<Option<PathBuf>>) -> Result<Self, String> {
        recover_pending()?;
        let directory = journal_directory()?;
        crate::database::with_connection(|connection| Self::begin_on(connection, directory, action, sources, targets, false))
    }
    pub(crate) fn begin_reserved_copy(source: PathBuf, target: PathBuf) -> Result<Self, String> {
        recover_pending()?;
        crate::database::with_connection(|connection| Self::begin_on(connection, journal_directory()?, Action::Copy, vec![source], vec![Some(target)], true))
    }
    fn begin_on(connection: &mut Connection, directory: PathBuf, action: Action, sources: Vec<PathBuf>, targets: Vec<Option<PathBuf>>, reserved_copy: bool) -> Result<Self, String> {
        if sources.is_empty() || sources.len() != targets.len() { return Err("Invalid catalog operation sources/targets.".into()); }
        let claimed: Vec<_> = sources.iter().cloned().chain(targets.iter().flatten().cloned()).collect();
        if live().lock().map_err(|_| "Catalog operation state unavailable")?.values().flatten().any(|path| claimed.iter().any(|new| path.starts_with(new) || new.starts_with(path))) {
            return Err("Another file operation is using one of these paths. Wait for it to finish.".into());
        }
        let mut observations = Vec::new();
        let targets_existed = targets.iter().map(|target| match target {
            Some(path) if !reserved_copy => match fs::symlink_metadata(path) {
                Ok(_) => Ok(true), Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false), Err(error) => Err(error.to_string()),
            }, _ => Ok(false),
        }).collect::<Result<Vec<_>, String>>()?;
        for (index, source) in sources.iter().enumerate() { collect(source, index, source, &mut observations)?; }
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let mut items = Vec::new();
        for (root, relative, before) in observations {
            let id = ensure_observed_on(&transaction, &before)?;
            let tagged: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM video_tags WHERE video_id=?1) OR EXISTS(SELECT 1 FROM videos WHERE id=?1 AND rating IS NOT NULL)", [id], |row| row.get(0)).map_err(|error| error.to_string())?;
            let content_hash = if action == Action::Trash && tagged { Some(crate::catalog::content_hash(Path::new(&before.path))?) } else { None };
            items.push(Item { id, root, before, relative, content_hash });
        }
        transaction.commit().map_err(|error| error.to_string())?;
        let id = format!("{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_nanos());
        let operation = Self { directory, plan: Plan { version: 1, id, action, sources, targets, targets_existed, items } };
        live().lock().map_err(|_| "Catalog operation state unavailable")?.insert(operation.plan.id.clone(), claimed);
        write_journal(&operation.directory, &operation.plan.id, "prepared", &operation.plan)?;
        Ok(operation)
    }

    fn completion(&self) -> Result<Completion, String> {
        let mut items = Vec::new();
        for item in &self.plan.items {
            let after = if self.plan.action == Action::Trash {
                match fs::symlink_metadata(&item.before.path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Ok(_) if observe(Path::new(&item.before.path))? == item.before => continue,
                    _ => return Err("A selected video changed during the Trash operation.".into()),
                }
            } else {
                let root = self.plan.targets[item.root].as_ref().ok_or("Missing catalog operation target")?;
                let path = if item.relative.as_os_str().is_empty() { root.clone() } else { root.join(&item.relative) };
                Some(observe(&path)?)
            };
            items.push(FinishedItem { item: item.clone(), after });
        }
        Ok(Completion { version: 1, id: self.plan.id.clone(), action: self.plan.action, items })
    }

    pub(crate) fn finish(&self) -> Result<(), String> {
        let result = (|| {
            let completion = self.completion()?;
            write_journal(&self.directory, &self.plan.id, "completed", &completion)?;
            crate::database::with_connection(|connection| apply_on(connection, &completion))?;
            remove_journals(&self.directory, &self.plan.id)
        })();
        if result.is_ok() { self.release(); }
        result.map_err(|error: String| format!("The file operation completed, but catalog updates are pending: {error}. Recovery information is at {}. Restart Framewise to retry; do not repeat the file operation.", self.directory.display()))
    }
    fn release(&self) { if let Ok(mut active) = live().lock() { active.remove(&self.plan.id); } }
    pub(crate) fn cancel_if_unchanged(&self) {
        if is_unchanged(&self.plan) {
            match remove_journals(&self.directory, &self.plan.id) { Ok(()) => self.release(), Err(error) => log::error!("Could not clear cancelled catalog operation: {error}") }
        } else { log::error!("Interrupted catalog operation requires review: {}", journal_path(&self.directory, &self.plan.id, "prepared").display()); }
    }
}
fn is_unchanged(plan: &Plan) -> bool {
    plan.items.iter().all(|item| observe(Path::new(&item.before.path)).is_ok_and(|video| video == item.before))
        && plan.targets.iter().enumerate().all(|(index, target)| plan.targets_existed.get(index).copied().unwrap_or(false) || target.as_ref().is_none_or(|path| matches!(fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound)))
}
impl Drop for Operation { fn drop(&mut self) { if let Ok(mut active) = live().lock() { active.remove(&self.plan.id); } } }

fn apply_on(connection: &mut Connection, completion: &Completion) -> Result<(), String> {
    if completion.version != 1 { return Err("Unsupported catalog operation journal version.".into()); }
    let transaction = connection.transaction().map_err(|error| error.to_string())?;
    let done: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM catalog_operation_receipts WHERE operation_id=?1)", [&completion.id], |row| row.get(0)).map_err(|error| error.to_string())?;
    if done { return Ok(()); }
    for finished in &completion.items {
        if (completion.action == Action::Trash) != finished.after.is_none() { return Err("Invalid catalog completion action/target.".into()); }
        let item = &finished.item;
        let known: Option<(String, i64, String)> = transaction.query_row("SELECT path,size,modified_ns FROM videos WHERE id=?1", [item.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(|error| error.to_string())?;
        if known != Some((item.before.path.clone(), i64::try_from(item.before.size).map_err(|error| error.to_string())?, item.before.modified_ns.clone())) {
            return Err("A source catalog record changed. Its assignments were retained for review.".into());
        }
        if let Some(after) = &finished.after {
            if observe(Path::new(&after.path))? != *after { return Err("A completed operation's destination has since changed. Assignments were retained for review.".into()); }
            // Preserve historical destination assignments rather than deleting records.
            transaction.execute("UPDATE videos SET status='changed' WHERE path=?1 AND status='active' AND id<>?2", params![after.path, item.id]).map_err(|error| error.to_string())?;
            let parent = Path::new(&after.path).parent().ok_or("Cannot read target parent")?.to_string_lossy();
            let size = i64::try_from(after.size).map_err(|error| error.to_string())?;
            if completion.action == Action::Copy {
                let kind = if crate::image_file(Path::new(&after.path)) { "image" } else { "video" };
                transaction.execute("INSERT INTO videos(path,parent_path,name,size,modified_ns,status,last_seen_at,media_kind,created_at) VALUES (?1,?2,?3,?4,?5,'active',unixepoch()*1000,?6,?7)", params![after.path, parent, after.name, size, after.modified_ns, kind, crate::catalog::created_at(Path::new(&after.path))]).map_err(|error| error.to_string())?;
                let new_id = transaction.last_insert_rowid();
                transaction.execute("UPDATE videos SET rating=(SELECT rating FROM videos WHERE id=?2) WHERE id=?1", params![new_id, item.id]).map_err(|error| error.to_string())?;
                transaction.execute("INSERT INTO video_tags(video_id,tag_id) SELECT ?1,tag_id FROM video_tags WHERE video_id=?2", params![new_id, item.id]).map_err(|error| error.to_string())?;
            } else {
                transaction.execute("UPDATE videos SET path=?1,parent_path=?2,name=?3,size=?4,modified_ns=?5,status='active',last_seen_at=unixepoch()*1000,content_hash=CASE WHEN ?7 THEN NULL ELSE content_hash END WHERE id=?6", params![after.path, parent, after.name, size, after.modified_ns, item.id, completion.action == Action::Replace]).map_err(|error| error.to_string())?;
            }
            transaction.execute("UPDATE videos SET created_at=?1 WHERE path=?2 AND status='active'", params![crate::catalog::created_at(Path::new(&after.path)), after.path]).map_err(|error| error.to_string())?;
        } else {
            match fs::symlink_metadata(&item.before.path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                _ => return Err("A trashed path is present again or inaccessible. Its assignments were retained for review.".into()),
            }
            transaction.execute("UPDATE videos SET status='trashed',content_hash=?2 WHERE id=?1", params![item.id, item.content_hash]).map_err(|error| error.to_string())?;
        }
    }
    transaction.execute("INSERT INTO catalog_operation_receipts(operation_id,completed_at) VALUES (?1,unixepoch()*1000)", [&completion.id]).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())
}

pub(crate) fn recover_pending() -> Result<(), String> {
    let directory = journal_directory()?;
    crate::database::with_connection(|connection| recover_on(connection, &directory))
}
fn recover_on(connection: &mut Connection, directory: &Path) -> Result<(), String> {
    if !directory.exists() { return Ok(()); }
    let active = live().lock().map_err(|_| "Catalog operation state unavailable")?.clone();
    let mut errors = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(id) = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_suffix(".prepared.json")) else { continue };
        if active.contains_key(id) { continue; }
        let result = (|| {
            let completed = journal_path(directory, id, "completed");
            if completed.exists() {
                let completion: Completion = serde_json::from_slice(&fs::read(&completed).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
                if completion.id != id { return Err("Journal identity mismatch.".into()); }
                apply_on(connection, &completion)?;
            } else {
                let done: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_operation_receipts WHERE operation_id=?1)", [id], |row| row.get(0)).map_err(|error| error.to_string())?;
                if !done {
                    let plan: Plan = serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
                    if plan.version != 1 || plan.id != id { return Err("Unsupported or mismatched operation journal.".into()); }
                    if !is_unchanged(&plan) {
                        return Err(format!("Interrupted operation is ambiguous; review {}. Original tag assignments were retained.", path.display()));
                    }
                }
            }
            remove_journals(directory, id)
        })();
        if let Err(error) = result { errors.push(error); }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

#[cfg(test)]
#[path = "catalog_operation_tests.rs"]
mod tests;
