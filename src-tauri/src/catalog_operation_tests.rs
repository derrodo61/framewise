use super::*;
use crate::database::{open_at, tests::Fixture};

fn seed(connection: &mut Connection, path: &Path) -> i64 {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"original video").unwrap();
    let transaction = connection.transaction().unwrap();
    let id = ensure_observed_on(&transaction, &observe(path).unwrap()).unwrap();
    transaction.execute("INSERT OR IGNORE INTO tags(name,normalized_name) VALUES ('Keeper','keeper')", []).unwrap();
    transaction.execute("INSERT INTO video_tags(video_id,tag_id) SELECT ?1,id FROM tags WHERE normalized_name='keeper'", [id]).unwrap();
    transaction.commit().unwrap();
    id
}
fn start(connection: &mut Connection, fixture: &Fixture, action: Action, sources: Vec<PathBuf>, targets: Vec<Option<PathBuf>>) -> Operation {
    Operation::begin_on(connection, fixture.folder.join("journal"), action, sources, targets, false).unwrap()
}
fn finish(connection: &mut Connection, operation: &Operation) {
    let completion = operation.completion().unwrap();
    write_journal(&operation.directory, &operation.plan.id, "completed", &completion).unwrap();
    apply_on(connection, &completion).unwrap();
    remove_journals(&operation.directory, &operation.plan.id).unwrap();
    operation.release();
}
fn active_id(connection: &Connection, path: &Path) -> i64 {
    connection.query_row("SELECT id FROM videos WHERE path=?1 AND status='active'", [path.canonicalize().unwrap().to_string_lossy().as_ref()], |row| row.get(0)).unwrap()
}
fn tags(connection: &Connection, id: i64) -> i64 {
    connection.query_row("SELECT COUNT(*) FROM video_tags WHERE video_id=?1", [id], |row| row.get(0)).unwrap()
}
#[test]
fn image_copy_move_and_trash_restore_keep_kind_and_assignments() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let source = fixture.folder.join("photo.jpg");
    let id = seed(&mut connection, &source);
    let copied = fixture.folder.join("photo (1).jpg");
    let copy = start(&mut connection, &fixture, Action::Copy, vec![source.clone()], vec![Some(copied.clone())]);
    fs::copy(&source, &copied).unwrap();
    finish(&mut connection, &copy);
    let copied_id = active_id(&connection, &copied);
    assert_ne!(copied_id, id);
    assert_eq!(tags(&connection, copied_id), 1);
    assert_eq!(connection.query_row("SELECT media_kind FROM videos WHERE id=?1", [copied_id], |row| row.get::<_, String>(0)).unwrap(), "image");
    let moved = fixture.folder.join("renamed.jpg");
    let movement = start(&mut connection, &fixture, Action::Move, vec![source.clone()], vec![Some(moved.clone())]);
    fs::rename(&source, &moved).unwrap();
    finish(&mut connection, &movement);
    assert_eq!(active_id(&connection, &moved), id);
    assert_eq!(tags(&connection, id), 1);
    let trash = start(&mut connection, &fixture, Action::Trash, vec![moved.clone()], vec![None]);
    let backup = fixture.folder.join("test-trash.jpg");
    fs::rename(&moved, &backup).unwrap();
    finish(&mut connection, &trash);
    assert_eq!(tags(&connection, id), 1);
    fs::rename(&backup, &moved).unwrap();
    assert_eq!(ensure_observed_on(&connection, &observe(&moved).unwrap()).unwrap(), id);
}
#[test]
fn cross_folder_trash_tracks_partial_success_without_losing_remaining_tags() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let first = fixture.folder.join("a/photo.jpg");
    let second = fixture.folder.join("b/clip.mp4");
    let first_id = seed(&mut connection, &first);
    let second_id = seed(&mut connection, &second);
    let operation = start(&mut connection, &fixture, Action::Trash, vec![first.clone(), second.clone()], vec![None, None]);
    fs::rename(&first, fixture.folder.join("simulated-trash.jpg")).unwrap();
    finish(&mut connection, &operation);
    assert_eq!(connection.query_row("SELECT status FROM videos WHERE id=?1", [first_id], |row| row.get::<_, String>(0)).unwrap(), "trashed");
    assert_eq!(active_id(&connection, &second), second_id);
    assert_eq!(tags(&connection, first_id), 1);
    assert_eq!(tags(&connection, second_id), 1);
}
fn status(connection: &Connection, id: i64) -> String {
    connection.query_row("SELECT status FROM videos WHERE id=?1", [id], |row| row.get(0)).unwrap()
}

#[test]
fn video_and_nested_folder_moves_keep_ids_and_tags() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let old = fixture.folder.join("source/nested/clip.mp4");
    let id = seed(&mut connection, &old);
    let folder = fixture.folder.join("source");
    let moved = fixture.folder.join("moved");
    let operation = start(&mut connection, &fixture, Action::Move, vec![folder.clone()], vec![Some(moved.clone())]);
    fs::rename(&folder, &moved).unwrap();
    finish(&mut connection, &operation);
    let new = moved.join("nested").join("clip.mp4");
    assert_eq!(active_id(&connection, &new), id);
    assert_eq!(tags(&connection, id), 1);
    let renamed = new.with_file_name("renamed.mp4");
    let rename = start(&mut connection, &fixture, Action::Move, vec![new.clone()], vec![Some(renamed.clone())]);
    fs::rename(&new, &renamed).unwrap();
    finish(&mut connection, &rename);
    assert_eq!(active_id(&connection, &renamed), id);
    assert_eq!(tags(&connection, id), 1);
}

#[test]
fn copy_save_as_and_save_have_the_intended_tag_identity() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let source = fixture.folder.join("clip.mp4");
    let id = seed(&mut connection, &source);
    for (name, bytes) in [("clip (1).mp4", b"original video".as_slice()), ("edited.mp4", b"shorter render".as_slice())] {
        let destination = fixture.folder.join(name);
        let operation = start(&mut connection, &fixture, Action::Copy, vec![source.clone()], vec![Some(destination.clone())]);
        fs::write(&destination, bytes).unwrap();
        finish(&mut connection, &operation);
        let copied_id = active_id(&connection, &destination);
        assert_ne!(copied_id, id);
        assert_eq!(tags(&connection, copied_id), 1);
    }
    let replacement = start(&mut connection, &fixture, Action::Replace, vec![source.clone()], vec![Some(source.clone())]);
    fs::write(&source, b"new edited video contents").unwrap();
    finish(&mut connection, &replacement);
    assert_eq!(active_id(&connection, &source), id);
    assert_eq!(tags(&connection, id), 1);
    assert_eq!(ensure_observed_on(&connection, &observe(&source).unwrap()).unwrap(), id);
}

#[test]
fn partial_trash_retains_tags_and_recognizes_restored_content() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let first = fixture.folder.join("first.mp4");
    let second = fixture.folder.join("second.mp4");
    let first_id = seed(&mut connection, &first);
    let second_id = seed(&mut connection, &second);
    let operation = start(&mut connection, &fixture, Action::Trash, vec![first.clone(), second.clone()], vec![None, None]);
    fs::remove_file(&first).unwrap();
    finish(&mut connection, &operation);
    assert_eq!(status(&connection, first_id), "trashed");
    assert_eq!(status(&connection, second_id), "active");
    assert_eq!(tags(&connection, first_id), 1);
    fs::write(&first, b"original video").unwrap();
    assert_eq!(ensure_observed_on(&connection, &observe(&first).unwrap()).unwrap(), first_id);
    assert_eq!(status(&connection, first_id), "active");
    let trash_again = start(&mut connection, &fixture, Action::Trash, vec![first.clone()], vec![None]);
    fs::remove_file(&first).unwrap();
    finish(&mut connection, &trash_again);
    fs::write(&first, b"different clip").unwrap(); // Same size, different content.
    let unrelated = ensure_observed_on(&connection, &observe(&first).unwrap()).unwrap();
    assert_ne!(unrelated, first_id);
    assert_eq!(tags(&connection, unrelated), 0);
    assert_eq!(tags(&connection, first_id), 1);
}

#[test]
fn failed_database_update_is_recovered_once_after_reopening() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let source = fixture.folder.join("clip.mp4");
    let destination = fixture.folder.join("copy.mp4");
    let source_id = seed(&mut connection, &source);
    let operation = start(&mut connection, &fixture, Action::Copy, vec![source.clone()], vec![Some(destination.clone())]);
    fs::copy(&source, &destination).unwrap();
    let completion = operation.completion().unwrap();
    write_journal(&operation.directory, &operation.plan.id, "completed", &completion).unwrap();
    connection.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON catalog_operation_receipts BEGIN SELECT RAISE(ABORT,'simulated database failure'); END;").unwrap();
    assert!(apply_on(&mut connection, &completion).is_err());
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM videos", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    let journal = operation.directory.clone();
    drop(operation); drop(connection);
    let mut reopened = open_at(&fixture.path).unwrap();
    reopened.execute_batch("DROP TRIGGER fail_receipt;").unwrap();
    recover_on(&mut reopened, &journal).unwrap();
    let copied_id = active_id(&reopened, &destination);
    assert_ne!(copied_id, source_id);
    assert_eq!(tags(&reopened, copied_id), 1);
    // Applying the same durable completion again cannot create another copy or assignment.
    apply_on(&mut reopened, &completion).unwrap();
    recover_on(&mut reopened, &journal).unwrap();
    assert_eq!(reopened.query_row("SELECT COUNT(*) FROM videos", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
    assert_eq!(reopened.query_row("SELECT COUNT(*) FROM catalog_operation_receipts", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(fs::read_dir(&journal).unwrap().count(), 0);
}

#[test]
fn cancelled_collision_keeps_assignments_and_clears_journal() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let source = fixture.folder.join("clip.mp4");
    let destination = fixture.folder.join("existing.mp4");
    let id = seed(&mut connection, &source);
    let other = seed(&mut connection, &destination);
    let operation = start(&mut connection, &fixture, Action::Move, vec![source.clone()], vec![Some(destination.clone())]);
    operation.cancel_if_unchanged();
    assert_eq!(active_id(&connection, &source), id);
    assert_eq!(active_id(&connection, &destination), other);
    assert_eq!(tags(&connection, id), 1);
    assert_eq!(fs::read_dir(&operation.directory).unwrap().count(), 0);
}

#[test]
fn ambiguous_copy_and_changed_completed_destination_are_not_guessed() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let source = fixture.folder.join("clip.mp4");
    let destination = fixture.folder.join("copy.mp4");
    let id = seed(&mut connection, &source);
    let operation = start(&mut connection, &fixture, Action::Copy, vec![source.clone()], vec![Some(destination.clone())]);
    fs::copy(&source, &destination).unwrap();
    let journal = operation.directory.clone();
    let completion = operation.completion().unwrap();
    drop(operation);
    assert!(recover_on(&mut connection, &journal).unwrap_err().contains("ambiguous"));
    assert_eq!(tags(&connection, id), 1);
    write_journal(&journal, &completion.id, "completed", &completion).unwrap();
    fs::write(&destination, b"unrelated replacement").unwrap();
    assert!(recover_on(&mut connection, &journal).is_err());
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM videos", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(tags(&connection, id), 1);
    assert!(journal_path(&journal, &completion.id, "prepared").exists());
}

#[test]
fn overlapping_live_operations_are_rejected() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let source = fixture.folder.join("clip.mp4");
    seed(&mut connection, &source);
    let operation = start(&mut connection, &fixture, Action::Copy, vec![source.clone()], vec![Some(fixture.folder.join("copy.mp4"))]);
    assert!(ensure_paths_idle(std::slice::from_ref(&source)).is_err());
    assert!(Operation::begin_on(&mut connection, fixture.folder.join("journal"), Action::Trash, vec![source], vec![None], false).is_err());
    operation.cancel_if_unchanged();
    assert!(ensure_paths_idle(&operation.plan.sources).is_ok());
}

#[test]
fn batch_moves_preserve_each_identity_even_when_copying_changes_file_times() {
    let fixture = Fixture::new();
    let mut connection = open_at(&fixture.path).unwrap();
    let first = fixture.folder.join("first.mp4");
    let second = fixture.folder.join("second.mp4");
    let first_id = seed(&mut connection, &first);
    let second_id = seed(&mut connection, &second);
    let directory = fixture.folder.join("destination");
    fs::create_dir(&directory).unwrap();
    let new_first = directory.join("first.mp4");
    let new_second = directory.join("second.mp4");
    let operation = start(&mut connection, &fixture, Action::Move, vec![first.clone(), second.clone()], vec![Some(new_first.clone()), Some(new_second.clone())]);
    fs::rename(&first, &new_first).unwrap();
    // Exercise the identity outcome of a cross-volume copy/remove fallback.
    fs::copy(&second, &new_second).unwrap();
    fs::OpenOptions::new().write(true).open(&new_second).unwrap().set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(123))).unwrap();
    fs::remove_file(&second).unwrap();
    finish(&mut connection, &operation);
    assert_eq!(active_id(&connection, &new_first), first_id);
    assert_eq!(active_id(&connection, &new_second), second_id);
    assert_eq!(tags(&connection, first_id), 1);
    assert_eq!(tags(&connection, second_id), 1);
    assert_eq!(ensure_observed_on(&connection, &observe(&new_second).unwrap()).unwrap(), second_id);
}
