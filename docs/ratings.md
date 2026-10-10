# Media ratings

Images and videos can have a rating from 1 to 5 stars. Unrated is stored as SQL NULL; Clear removes the rating. Ratings stay in Framewise's local catalog and do not modify media files.

- Inspector: click a star to assign that value; Clear returns the file to Unrated.
- List and grid: assigned ratings appear beside or below the name.
- Context menu: Rate… applies a rating to the selected files together. Mixed ratings are labelled until a common value is chosen. Changes save immediately.
- Filters: All ratings, Unrated only, Exactly, At least, and More than. More than 3 returns 4 and 5; At least 3 includes 3. Unrated is excluded from numeric comparisons. More than 5 has no file matches.
- Rating filters combine with date, media type and tag filters in both folder and workspace scope. Workspace filtering occurs before counts and pagination. Folder entries remain available for navigation.
- The rating filter is remembered in preferences. Select all and Trash respect it; Trash also checks current catalog ratings before deleting a batch.

Catalog schema 6 adds a nullable, constrained integer rating. Existing files start unrated. Migration makes a consistent database backup first and preserves tags, previews and media identities.

Rename, move and Save preserve the media identity and rating. Duplicate and Save as copy the source rating. Trash retains ratings for content-verified restoration, including files without tags. Unrelated replacements do not inherit ratings.

Verification: frontend build/lint/tests, 71 Rust tests and Clippy. Regression coverage includes migration/persistence, invalid values, stale batch rollback, combined filtering/pagination, filtered selection/Trash, copy/edit preservation, move and Trash restoration.

Manual checks:

1. Rate a video 3 and an image 5; check list/grid stars and restart persistence.
2. Compare Exactly 3, At least 3, More than 3 and Unrated only. Select Videos alone to show only unrated videos.
3. Repeat in Workspace scope with date/tag filters.
4. Select files with different ratings, choose Rate… and assign a common rating; clear it again.
