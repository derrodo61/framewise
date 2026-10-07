# Framewise

A local desktop app for browsing folders and inspecting video metadata. Built with Tauri 2, React, TypeScript, and `ffprobe`.

## What it does

- Show the installed build's version beside the app name and in the video editor (currently 0.1.22).
- Choose a folder and browse its subfolders and supported video files.
- Switch between a compact List and a Grid with video thumbnails and filenames. The view choice is saved; grid thumbnails load near the visible area and reuse the preview cache.
- Choose Small, Medium, or Large previews in Grid view. The size is saved and changes immediately using the cached images.
- Sort either view by filename or date modified, ascending or descending. Folders stay first; sorting choices are saved.
- Create a subfolder directly from the media file list.
- Click a video to see container, video, audio, and embedded tag metadata.
- See and copy positive and negative generation prompts in the Inspector. Embedded WAN2GP JSON and ComfyUI execution graphs are detected; ComfyUI extraction follows active output connections. Unknown formats, missing prompts, and unsupported text transformations are explained. Sidecar files and workflow-only prompt reconstruction are not supported yet.
- ComfyUI Crystools prompt switches follow the selected manual/automatic branch. Easy Use display nodes can supply generated text saved in the embedded workflow; this text is labelled with its saved source. If generated output is missing, Framewise explains the limitation instead of substituting a prompt idea or an inactive manual prompt. MiniMax H3 Reference-to-Video conditioning is supported.
- Recorded seeds appear below the prompt with Copy seed buttons. WAN2GP seed fields and active ComfyUI sampler/noise nodes are supported, including linked integer seeds. Video seeds and automatic prompt-generator seeds are labelled separately with their source. Large integers keep their exact digits; random seed placeholders are not presented as the actual generation seed.
- See a thumbnail in the Inspector and click Play to watch the video in the same space.
- Double-click a video in List or Grid view to start it in the Inspector. Press Enter on a focused video to toggle playback between playing and paused. A collapsed Inspector opens automatically.
- Right-click a video to move it to the system Trash or Recycle Bin after confirmation.
- With focus in the file list, press Delete (or Backspace on macOS) to open the Trash confirmation for the selected videos, including multiple selections.
- Right-click a subfolder to move it and its contents to Trash after confirmation.
- Rename a video or subfolder from its context menu. Video extensions stay unchanged.
- Reveal a video or subfolder in Explorer (Windows), Finder (macOS), or the system file manager (Linux) from its context menu.
- Select several videos with Ctrl-click or Shift-click (Command-click on macOS), then right-click a selected video and choose Move to. The separate destination window can browse folders, jump to another location, create a folder, and move the selected videos there.
- Right-click a folder and choose Move to, or Ctrl/Command-click or Shift-click several folders to move them together with all their contents. A normal click still opens a folder. Folder moves reject existing destination names and destinations inside the selected folders; cached preview associations follow the move. Moving folders between drives copies and verifies the entire tree before removing the source; links and special files in such trees are rejected.
- Open the editor from the Inspector, scrub through a video, mark one section to remove, preview the cut, and undo it. Save As creates an MP4; Save replaces an MP4 after rendering and verification.
- Use the Up and Down arrow keys to move through the file list, including folders and “Go back.” Press Enter to open a focused folder.
- In Grid view, use Left and Right to move between cards, and Up and Down to move between rows.
- Collapse either side panel to give the video browser more room. Panel choices are saved locally.
- Drag the dividers to resize Workspace and Inspector, or focus a divider and use the arrow keys. Widths are saved locally.
- Set a startup folder in Settings so it opens automatically the next time the app starts.
- Choose a light or dark theme in Settings. Window size and position are restored when the app reopens.
- Manage reusable tags in Settings → Tags: create, search, rename, and delete tags locally. Deleting a tag removes its assignments, never video files.
- Add/remove tags in the Inspector, or select several videos and choose Edit tags from the context menu. Batch editing distinguishes tags assigned to all selected videos from those assigned to only some. Creating while assigning reuses an existing equivalent name. Assignments are saved locally without modifying video files.
- Select several existing tags using checkboxes, optionally narrow the list with Find tags, then Add selected. In batch editing, all chosen tags are added to all selected videos together. Selections remain checked while searching; Clear selection resets the picker.
- Filter the current folder by one or more tags with Match all or Match any. List and Grid share the same results, with a count and Clear filters action. Folders remain available for navigation. Filters follow folder browsing within a workspace and reset when changing workspace.
- Choose Entire workspace to discover videos recursively with scan progress and cancellation, then search by tags across subfolders. Results show folder locations and load in pages of 200. Use Open containing folder to resume ordinary folder browsing. Workspace filename sorting is alphabetical; current-folder sorting uses natural filename order.
- Workspace scanning reads file details without generating previews or probing every video. Grid previews load near visible results. Scans pause file changes and tag assignments until completion/cancellation. Cross-folder batch tagging is supported; Move To and Trash batches require videos from one folder. Scans report inaccessible folders and preserve their catalog records, with limits of 10,000 entries per folder and 100,000 entries per workspace.
- View or copy the full `ffprobe` JSON output.
- Files are read locally; the app does not upload them.

## Run locally

Install [Node.js](https://nodejs.org/), the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS, and [FFmpeg](https://ffmpeg.org/download.html) (which includes `ffprobe`). Make sure `ffmpeg` and `ffprobe` are on `PATH`, or set `FFMPEG_PATH` and `FFPROBE_PATH` to their full executable paths.

```sh
npm install
npm run tauri dev
```

Run `npm test` for frame navigation and `cargo test --manifest-path src-tauri/Cargo.toml` for Rust checks, including real FFmpeg exports. The Rust export tests require `ffmpeg` and `ffprobe` as described above.

GitHub Actions runs these checks on Windows, macOS, and Linux for pushes to `main` or `feature/video-editor` and for pull requests. Video playback in each operating system's WebView still needs a manual check.

Build an installer on the target OS with `npm run tauri build`. Build and test on Windows, macOS, and Linux separately. The current prototype expects a local `ffprobe` installation; packaging it with the app is a later distribution step and requires checking the FFmpeg build's license terms.

Framewise saves the startup folder, theme, panel layout, and window state in `~/.framewise` (`settings.json` and `window-state.json`). These files are shared by development and installed builds and remain in the user's home folder when the app is upgraded or uninstalled. On first launch, Framewise imports preferences from its earlier WebView storage and copies the previous window state when available.

Video preview images are generated when needed and saved in `~/.framewise/previews`. A SQLite index at `~/.framewise/framewise.db` connects each video path and file version to its image. Existing images are added to the index when their videos are opened. On startup, Framewise moves previews from its earlier cache folder into `~/.framewise/previews`. You can delete preview images to reclaim space; Framewise will regenerate them as needed. Both locations are shown in Settings.

The video catalog and tag system is available on `main`; see [the roadmap](docs/video-tags-and-search-plan.md) for its implementation stages. Video records and tag tables share the database but are independent of the preview cache. Before upgrading an existing catalog, Framewise writes a consistent `framewise-before-catalog-v<schema>-<timestamp>.db` snapshot alongside it. The preview schema remains compatible with 0.1.21. In-app moves, renames, and Save preserve IDs and assignments; Duplicate and Save As create separate records with copied tags. Tagged videos restored from Trash can be recognized by a unique content-hash match at the old path. Pending catalog updates are journaled in `~/.framewise/catalog-operations` and retried on startup; ambiguous interruptions retain records for review and show a browser warning. Tag management, assignment, and folder/workspace filtering are available; general external-move reconciliation remains deferred.

If `ffprobe` works in a new terminal but Framewise cannot find it, restart the terminal that launches `npm run tauri dev` so it picks up your updated `PATH`. On Windows, Framewise also looks in Windows Package Manager's FFmpeg installation folder. `FFPROBE_PATH` takes precedence over both locations.

## Error logs

Framewise records app errors in a local log file, with timestamps. The log is limited to 1 MB and keeps two older files. Find it in the app's log directory:

- Windows: `%LOCALAPPDATA%\app.framewise.desktop\logs`
- macOS: `~/Library/Logs/app.framewise.desktop`
- Linux: `~/.local/share/app.framewise.desktop/logs` (or under `$XDG_DATA_HOME`)

Logs can include local file paths and error details. Review them before sharing them with anyone.

## Notes

The selected folder limits which source videos the backend will inspect or edit. Symbolic links are hidden. Preview thumbnails are cached locally. Inline playback depends on the codecs supported by the operating system's WebView; metadata and the thumbnail can still work when playback cannot.

The first editor supports one video track, up to one audio track, and no chapters or other tracks. It re-encodes to 8-bit H.264/AAC in MP4, so quality, HDR appearance, and technical metadata can change. Frame stepping uses the source's presentation timestamps, including variable frame spacing. The cut preview skips over the marked section during playback; the saved MP4 is rendered separately by FFmpeg. Descriptive container tags are copied and checked before saving; changed track tags are reported after saving. Output duration is checked against the expected cut. Save As requires a new file name and never overwrites an existing file. Save renders beside the original, verifies the output, and keeps the previous version in Trash; if Trash is unavailable, the app reports the backup path instead. Keep the app open while rendering.
