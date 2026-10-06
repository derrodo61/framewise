# Video tags and search

## Goal

Make a growing video collection easy to organize and find without relying only on folder names. Users can manage tags, assign or remove them from individual videos or batches, and find tagged videos across their workspace in either List or Grid view.

This document is the implementation roadmap for `codex/video-tags-and-search`, based on `main` at `1bd2f08`. Implement and test one phase at a time, then review the result before beginning the next phase.

## First release scope

- Manual tags stored locally in Framewise's SQLite database.
- Create, rename, and delete tags.
- Assign and remove tags on one or several selected videos.
- Filter using one or more tags, with Match all / Match any.
- Search scope: Current folder or Entire workspace, including subfolders.
- Results in List and Grid, with folder location visible for workspace results.
- Keep existing preview, playback, keyboard navigation, sorting, and file actions working.

The first release organizes videos. Photo support, automatic tags, filename/date/generator/prompt filters, and reliable discovery of files moved outside Framewise are later extensions.

## Product decisions

- Tags belong to video records, not thumbnail files. Clearing the preview cache must never remove tags.
- Tags are shared across workspaces in the user's local database, not embedded into video metadata.
- Trim tag names, reject empty names, and prevent case-insensitive duplicate names. Preserve the user's display spelling and support Unicode.
- Deleting a tag requires confirmation and removes its assignments, never the videos.
- Adding a tag to a batch adds it to all selected videos. Removing it removes it from all selected videos. Existing unrelated tags remain unchanged.
- Batch controls distinguish tags assigned to all selected videos from tags assigned to only some.
- Tag filters select videos; folders are not taggable in this release. Keep folder navigation available separately from the result list.
- Current folder means direct children only. Entire workspace means supported videos under the chosen workspace root, including nested folders.
- An empty tag filter shows all videos in the selected scope. No matches shows a clear empty state with a Clear filters action.
- Start with Current folder and Match all. Preserve the view, sorting, and grid-size preferences. Reset active tag filters when changing workspace so a new workspace does not unexpectedly appear empty.
- In-app moves and renames preserve video IDs and tags, including nested videos when moving or renaming a folder.
- Save retains the video's tags. Duplicate and Save As create a new video record and copy the source's tag assignments.
- Retain records and assignments for videos moved to Trash or temporarily missing, but exclude them from normal search. A reused path must not silently give an unrelated video the previous video's tags.

## Current implementation

- `~/.framewise/framewise.db` currently contains the preview index, schema version 1.
- Preview associations use the video path and a file version derived from its size and modification time.
- Existing move, rename, edit, duplicate, and Trash operations already contain preview lifecycle integration.
- Directory browsing currently enumerates one folder. Workspace-wide searching needs an index and recursive discovery.
- ComfyUI and WAN2GP prompt extraction uses the inspected metadata; it is not a searchable metadata catalog yet.

## Proposed data model

Extend the existing database with tables equivalent to:

| Table | Purpose |
| --- | --- |
| `videos` | Stable ID, canonical path, filename, size, modification time, discovery/status information |
| `tags` | Stable ID, display name, normalized unique name |
| `video_tags` | Unique video/tag pairs with foreign keys and indexes |
| Workspace scan state | Track completed scans and distinguish missing files from incomplete or failed scans |

Use paths and file versions as discovery information, not as permanent identity. Move/rename operations performed by Framewise explicitly transfer the known ID. Size and timestamps alone are insufficient to identify an externally moved video reliably.

Prefer one shared database layer with versioned migrations, transactions, foreign-key enforcement, and parameterized queries. Preserve the existing preview records and cache behavior during migration. Final field names and scan-state representation will be chosen in Phase 1.

## Phases and checkpoints

### Phase 0 — Plan

Status: Complete.

- Create the feature branch and this roadmap.
- Keep runtime behavior unchanged until Phase 1 is authorized.

### Phase 1 — Database foundation and stable video records

Status: Complete. Automated checks passed; the user confirmed development-mode browsing, cached previews, and restarting work.

Implementation decisions:

- Shared initialization and connection ownership live in `src-tauri/src/database.rs`; preview queries use that connection.
- SQLite `user_version` remains 1 and the `previews` table is unchanged, preserving the installed 0.1.21 app's preview access. New catalog migrations use `catalog_schema.version`, currently 1. A compatibility test reads and writes previews through a legacy connection without the new tag validation function.
- Before migrating an existing database, SQLite creates a consistent snapshot (including committed WAL data) named `~/.framewise/framewise-before-catalog-v1-<timestamp>.db`. The snapshot is flushed before migration, and its path is logged. Successful catalog initialization does not repeat this backup on subsequent launches.
- Tables cover videos, tags, assignments, completed folder discovery, and future workspace scan runs. Assignments enforce foreign keys and unique video/tag pairs.
- Normal folder opening/refresh registers direct-child videos, returning optional `videoId` values. Discovery uses existing filesystem metadata, with nanosecond modification times, and does not run ffprobe or generate thumbnails. File-operation response listings will be integrated in Phase 2.
- Unchanged active records retain their IDs. A changed size or modification time retires the old record as `changed` and creates a new, untagged record. Absent direct children become `missing` only after a successful listing. Present files with unreadable metadata and files in other folders are not marked missing.
- Reappearing paths get new IDs rather than silently inheriting historical assignments. Size/time are only identity hints: unrelated replacements with identical hints cannot be detected yet. Explicit in-app identity preservation belongs to Phase 2; external reconciliation remains deferred.
- Tag display names preserve trimmed spelling, allow 1–100 characters, and reject control characters. Unique keys use Unicode normalization and full case folding, including composed/decomposed accents and `Straße` / `STRASSE` equivalence. Database constraints validate keys using the shared connection's registered SQL function.
- Preview cache removal does not affect catalog records or assignments. Unsupported schemas and migration failures leave existing records intact; migrations are transactional.

Validation:

- Rust tests cover WAL-aware backups, reopening, legacy preview compatibility, rollback on migration failure, Unicode validation, duplicate assignments, foreign keys, stable discovery IDs, retained history, missing metadata, folder isolation, and transaction rollback for an invalid batch.
- Existing preview lifecycle and filesystem/editor tests continue to pass.
- Frontend production build, lint, and existing tests pass. No installer or version bump was created.
- Manual development-mode checks for browsing, cached preview responsiveness, and restarting were confirmed by the user. Installed 0.1.21 compatibility was verified through the database compatibility test; a manual installed-app check and manual inspection of the backup file were not reported.

Manual checkpoint:

1. Start the development app and browse a folder; verify the existing file list and Inspector behave as before.
2. Select previously previewed videos; check that cached previews remain fast.
3. Restart the development app and repeat browsing and previewing.
4. Check `.framewise` for the pre-migration snapshot after the first successful upgrade of an existing database.
5. Optionally run the installed 0.1.21 app and confirm its previews still work. Its file actions do not maintain catalog identities yet; Phase 2 integrates those actions in the development app.

This phase adds backend foundations only; tag management and assignment controls arrive in Phases 3 and 4.

Work:

- Separate shared database initialization/migrations from preview-specific queries.
- Add video, tag, and assignment tables, indexes, and validated tag-name normalization.
- Register videos discovered through ordinary directory browsing without probing every file with ffprobe.
- Define handling for known-path changes, missing records, and path reuse; avoid silently transferring assignments to unrelated content.
- Make migrations transactional and repeatable. Coordinate schema version handling across every database consumer.

Checkpoint:

- Tests migrate a representative version-1 database without losing preview entries.
- Reopening the database preserves IDs and assignments; duplicate tags and assignments are rejected.
- Existing cached previews, preferences, and directory browsing still work.
- Test against temporary database fixtures first. Before migrating the user's shared database, provide a recoverable backup and address older installed builds' schema compatibility. Development and installed builds share this database today.

### Phase 2 — File-operation identity and tag preservation

Status: Complete. Automated checks passed; the user completed the manual checklist and reported no problems. Depends on Phase 1.

Implementation decisions:

- Moves and renames retain IDs and assignments, including supported videos discovered recursively inside moved/renamed folders. Source records are prepared before filesystem changes.
- Save updates the original record's file version and retains its ID. Duplicate and Save As explicitly create separate records and copy the source's assignments. File-operation response listings now register catalog IDs.
- Trash retains IDs and assignments with `trashed` status; partial batch failures update only videos actually absent afterward. Existing preview relocation/cleanup remains in place.
- Tagged videos receive a SHA-256 content identity before Trash. A file restored at its old path reuses a trashed record only when its size and content hash uniquely match. An unrelated replacement or multiple matching historical records receives a new ID. Untagged videos are not read for hashing during deletion; external moves and general missing-file reconciliation remain deferred.
- The catalog schema is now version 3: operation receipts and optional content hashes are additive migrations. SQLite `user_version` and the preview schema remain unchanged. Existing databases receive a consistent `framewise-before-catalog-v3-<timestamp>.db` snapshot before upgrading.
- `~/.framewise/catalog-operations` holds flushed preparation/completion JSON journals. Completed filesystem operations can replay catalog transactions after restart; receipts make replay idempotent. Successful or safely canceled operations remove their journals.
- Operations with overlapping source/destination paths are rejected while another operation is using them. Preparation captures filenames, sizes, and modification times without ffprobe or thumbnail generation.
- If filesystem work succeeds but catalog persistence fails, the app reports pending recovery rather than ordinary success. Ambiguous interruptions and destinations changed before recovery retain the journal and original assignments for manual review; they are not guessed from size/time. Catalog discovery is deferred in that situation and the browser shows a warning. No journal-review UI is included yet.
- Tag assignments in these tests are seeded into temporary databases. Tag management/assignment controls still arrive in Phases 3 and 4.

Validation:

- Tests cover nested folder moves, video renames, batch moves including changed timestamps after copy/remove transfers, separate Duplicate/Save As records, Save identity, partial Trash, restored tagged content, same-size unrelated replacements, canceled collisions, overlapping operations, database transaction failure, idempotent restart recovery, ambiguous interruption, and changed destinations.
- Schema-upgrade tests preserve Phase 1 IDs, assignments, and previews and verify the pre-upgrade snapshot.
- All 42 Rust tests passed, including the existing preview lifecycle, filesystem operations, and real FFmpeg export tests. Frontend build/lint/tests and Rust clippy passed.
- No installer or version bump was created. Actual multi-drive and Mac/Linux manual checks have not been performed in this phase.
- The user completed the Windows development-mode manual checklist for Duplicate, rename/move, folder operations, Save/Save As, Trash/cancel/restore, collisions, restarting, and previews, and reported no problems. Tag preservation remains verified through backend tests until the tag controls are implemented.

Manual checkpoint (use copies of test videos):

1. Duplicate a video, rename the duplicate, move it, and restart. Confirm the file list, metadata, and previews still work.
2. Rename and move a folder containing nested videos. Browse those videos afterward and after restarting.
3. Edit a test video and try both Save As and Save. Confirm output playback, metadata, and preview refresh.
4. Cancel a Trash dialog, then Trash one or several test copies. Verify the remaining list and restore a copy through the system Trash/Recycle Bin to check browsing again.
5. Attempt a move/rename into an existing name; verify it fails without moving anything unexpectedly.
6. Confirm previously cached previews remain responsive. Tag identity preservation is covered by backend tests until the tag UI exists.

Work:

- Integrate IDs and assignments with video/folder move and rename, Save, Save As, Duplicate, and Trash.
- Update descendant paths when a folder moves or changes name.
- Preserve the existing preview relocation and cleanup behavior.
- Handle filesystem success followed by database failure explicitly. Record enough recovery information to reconcile on restart rather than reporting a successful organization update prematurely.

Checkpoint:

- Filesystem tests verify tags survive single/batch moves, folder moves, renames, and Save.
- Save As and Duplicate produce separate records with copied assignments.
- Collision, cancellation, and rollback cases do not assign tags to the wrong path.
- Trash/missing records stay out of results; restoring a recognized record preserves its tags.
- Preview caching and generation metadata remain unchanged by these operations.

### Phase 3 — Manage tags

Status: Pending. Depends on Phase 1.

Work:

- Add an accessible Manage tags section to Settings.
- List and search tags; create, rename, and delete them.
- Show useful validation errors and confirm tag deletion with the affected video count.
- Show changes consistently wherever tags are displayed.

Checkpoint:

- Manually create, rename, and delete tags; restart and verify persistence.
- Test empty names, duplicate names, case differences, and Unicode.
- Renaming changes existing labels; deletion removes assignments without touching video files.

### Phase 4 — Single and batch tag assignment

Status: Pending. Depends on Phases 2 and 3.

Work:

- Add a Tags section in the Inspector with assigned tags, removal controls, and an existing-tag picker.
- Allow creating a new tag from the assignment flow.
- Add a context-menu action for tagging selected videos in List and Grid.
- Provide a batch panel/dialog when several videos are selected; do not treat the Inspector's last selected video as the entire batch.
- Indicate tags present on all versus only some selected videos.

Checkpoint:

- Assign and remove tags from one video and a batch; verify after restarting.
- Adding an existing tag is harmless; removing one leaves unrelated assignments intact.
- Test Ctrl/Command-click, Shift-click, keyboard selection, folder selections, and selection changes during an operation.
- Existing Inspector playback and prompt-copy actions still work.

### Phase 5 — Tag filtering in the current folder

Status: Pending. Depends on Phase 4.

Work:

- Add tag selection, Match all / Match any, active-filter indicators, result count, and Clear filters above the list.
- Query using video IDs and tag IDs, not rendered labels.
- Keep folder navigation separate and available while filtering videos.
- Ensure rendering, keyboard movement, range selection, and context menus use the same filtered and sorted sequence.
- Define selection behavior when filtering hides a selected video: clear hidden selection and Inspector content rather than leaving invisible action targets.

Checkpoint:

- Tests cover all/any combinations, no filters, no matches, renamed/deleted tags, and missing files.
- Manually verify both views, sorting, grid sizes, keyboard actions, and batch operations.
- A folder navigation action retains the active filter within the same workspace and clearly shows it is active.

### Phase 6 — Workspace discovery and search across subfolders

Status: Pending. Depends on Phase 5.

Work:

- Add Current folder / Entire workspace scope selection.
- Discover supported videos recursively in a background task, with progress, cancellation, and bounded work. Skip links consistently with existing browsing rules.
- Use indexed tag queries for results. Load thumbnails only near visible results; do not generate thumbnails or inspect every video's metadata during scanning.
- Mark missing records only after their relevant subtree was successfully scanned. Permission errors, cancellation, or an unavailable drive must not mark an entire collection deleted.
- Display result locations and keep Show in Explorer/Finder available.
- Generalize actions that currently assume one source folder: group workspace selections by parent where required, or clearly restrict unsupported cross-folder batches until they are implemented. Never submit a mixed-parent selection to commands that require a single parent.
- Ensure workspace discovery, scan responses, and selection do not leak into a subsequently chosen workspace.

Checkpoint:

- Scan fixtures with nested folders, duplicate filenames, inaccessible folders, removed files, and interrupted scans.
- Find matching videos across several subfolders and open the correct video despite identical filenames.
- Verify result actions and any cross-folder batch restrictions.
- Check responsiveness on a large collection and confirm existing cached previews remain fast.

### Phase 7 — Integrated review and release preparation

Status: Pending. Depends on Phase 6.

Work:

- Review database ownership, migration recovery, scan/query performance, and UI consistency.
- Document tag storage, backup implications, search scope, and external-move limitations.
- Review on Windows and perform Mac/Linux checks when those environments are available; record unverified platform behavior honestly.
- Run the applicable existing test/build/lint checks and targeted regression tests.

Checkpoint:

- Complete an end-to-end session: scan, create tags, batch-tag, filter across folders, move/rename, restart, and find the same videos again.
- Confirm light/dark themes, independent panel scrolling, saved layout, preview speed, prompt extraction, and playback shortcuts.
- Review the feature branch before merging into `main`.
- Build a Windows installer only when requested by the user.

## Later extensions

- Filename, date, generator, and prompt-text filters using the same search interface.
- Optional automatic tag suggestions, clearly distinguished from manual assignments.
- External move/rename reconciliation using stronger file identity evidence and user review for ambiguity.
- Photo support, saved searches, tag groups/colors, and tag import/export.

## Working process

1. Implement the next agreed phase, keeping it small enough to review.
2. Run its focused automated checks and provide a short manual testing checklist.
3. Record outcomes and decisions in this document, including any limitations.
4. Review with the user before proceeding to the next phase.
5. Commit/push when requested. Create an installer only when explicitly requested.
