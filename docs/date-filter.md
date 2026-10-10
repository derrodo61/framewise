# Date filtering

Implemented on `codex/image-support`; awaiting user testing.

- Modified date is the default; Created uses filesystem creation time, not embedded generation metadata.
- Presets: Today, Yesterday, Last 7 days, Last 30 days, All dates.
- Last 7/30 days include today. Custom From and To include whole local days; setting the same date selects that day. Either endpoint may be blank for an open-ended range.
- Boundaries use local calendar days, including 23/25-hour daylight-saving days. Presets update at midnight and when the window regains focus.
- Invalid/reversed dates show an error and no file matches. Folders stay available. Missing timestamps are excluded only when the relevant date filter is active.
- Dates combine with tags and media types. Workspace counts and pagination apply the date predicate before loading results. Criteria follow folder browsing and reset when changing workspace.
- Catalog schema 5 adds nullable creation timestamps with an automatic consistent backup. Previously catalogued creation timestamps fill when browsing or scanning; unknown dates are never invented. Rename/move/copy/restore refresh timestamps from the current filesystem.
- Inspector displays Created and Modified in local time.

Checks passed: frontend build/lint/tests; 62 Rust tests; Clippy with warnings denied. Date tests cover presets, same-day/open/custom ranges, invalid dates, inclusive lower/exclusive upper boundaries, DST changes, unknown creation dates, migration identity/tag preservation and workspace paging.

Manual checkpoint:

1. In a mixed folder, try Today/Yesterday/Last 7/30 days with Modified selected.
2. Select Custom dates, use the same From/To day, then expand the range and try leaving one endpoint blank.
3. Choose Created and compare the Inspector dates; scan the workspace to load older catalog records' creation dates.
4. Combine date, type and tag filters in List/Grid and Entire workspace; check counts, paging, selection clearing, and folder navigation.
5. Clear the filter and switch workspace; verify ordinary browsing and video playback still work.

No version bump, installer, or automatic commit.
