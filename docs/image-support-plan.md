# Image support

Branch: `codex/image-support`, based on `main`.

Status: Complete; user tested and reported it works. No version bump or installer.

Checks passed: frontend build, lint and tests; 60 Rust tests; Clippy with warnings denied; real JPG/PNG/WebP ffprobe dimension checks. Added coverage for migration preservation, mixed tagging, image copy/move/Trash restoration and type filtering before workspace pagination.

## 1. Browsing and previews

- Support JPG/JPEG, PNG and WebP alongside videos.
- Remember Videos/Images checkboxes; keep folders available.
- Show images in List/Grid and the Inspector. Enter selects/displays images; Space playback and video editing remain video-only.
- Load image previews only near visible grid cards.

## 2. Organization and search

- Extend existing catalog identities, tags, rename, duplicate, move and Trash to images, including mixed media selections.
- Apply type filters before workspace pagination, counts and selection.
- Add an additive database migration with a consistent backup. Preserve existing video IDs and tag assignments.

## 3. Image information

- Display native image dimensions and available ffprobe file/stream metadata without video playback fields.
- Image previews work without ffprobe. Malformed images show an explicit error.
- Image generator-specific prompt/seed extraction is a later extension; do not infer it from video metadata rules.

## Verification

- Test catalog migration, mixed discovery and workspace filtering/paging, image tag identity across operations, and keyboard/type filtering.
- Run frontend build/lint/tests and Rust tests/Clippy.
- Manual checkpoint: mixed folders in both views, each checkbox combination, image Inspector, tags and file operations, workspace search, keyboard navigation, restart preferences, existing video playback/editing.
- No installer or automatic commit for this feature.

## Implementation notes

- Catalog schema 4 adds indexed media kind to existing records, defaulting old records to video. IDs and assignments are preserved; migration writes a consistent `framewise-before-catalog-v4-<timestamp>.db` backup. The legacy table/command names remain internal compatibility details.
- Older installed versions that support only catalog schema 3 cannot use the upgraded catalog. Continue testing with the new dev build; a new installer can be created when requested.
- Image previews use the original file through the restricted asset protocol, with visibility-driven loading; they do not create another image cache. The browser reports actual image dimensions. Additional file/stream fields come from ffprobe, with video-only fields hidden.
- Metadata failures do not prevent image previews or tag editing. Full EXIF and generator-specific image prompts are not part of this first implementation.
