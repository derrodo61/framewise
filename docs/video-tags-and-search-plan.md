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

Status: Pending.

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

Status: Pending. Depends on Phase 1.

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
