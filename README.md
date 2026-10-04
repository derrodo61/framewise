# Framewise

A local desktop app for browsing folders and inspecting video metadata. Built with Tauri 2, React, TypeScript, and `ffprobe`.

## What it does

- Choose a folder and browse its subfolders and supported video files.
- Create a subfolder directly from the media file list.
- Click a video to see container, video, audio, and embedded tag metadata.
- See a thumbnail in the Inspector and click Play to watch the video in the same space.
- Right-click a video to move it to the system Trash or Recycle Bin after confirmation.
- Right-click a subfolder to move it and its contents to Trash after confirmation.
- Rename a video or subfolder from its context menu. Video extensions stay unchanged.
- Select several videos with Ctrl-click or Shift-click (Command-click on macOS), then right-click a selected video and choose Move to. The separate destination window can browse folders, jump to another location, create a folder, and move the selected videos there.
- Open the editor from the Inspector, scrub through a video, mark one section to remove, preview the cut, and undo it. Save As creates an MP4; Save replaces an MP4 after rendering and verification.
- Use the Up and Down arrow keys to move through the file list, including folders and “Go back.” Press Enter to open a focused folder.
- Collapse either side panel to give the video browser more room. Panel choices are saved locally.
- Drag the dividers to resize Workspace and Inspector, or focus a divider and use the arrow keys. Widths are saved locally.
- Set a startup folder in Settings so it opens automatically the next time the app starts.
- Choose a light or dark theme in Settings. Window size and position are restored when the app reopens.
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
