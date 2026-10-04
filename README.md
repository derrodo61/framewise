# Framewise

A local desktop app for browsing folders and inspecting video metadata. Built with Tauri 2, React, TypeScript, and `ffprobe`.

## What it does

- Choose a folder and browse its subfolders and supported video files.
- Click a video to see container, video, audio, and embedded tag metadata.
- Collapse either side panel to give the video browser more room. Panel choices are saved locally.
- Drag the dividers to resize Workspace and Inspector, or focus a divider and use the arrow keys. Widths are saved locally.
- View or copy the full `ffprobe` JSON output.
- Files are read locally; the app does not upload them.

## Run locally

Install [Node.js](https://nodejs.org/), the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS, and [FFmpeg](https://ffmpeg.org/download.html) (which includes `ffprobe`). Make sure `ffprobe` is on `PATH`, or set `FFPROBE_PATH` to the full executable path.

```sh
npm install
npm run tauri dev
```

Build an installer on the target OS with `npm run tauri build`. Build and test on Windows, macOS, and Linux separately. The current prototype expects a local `ffprobe` installation; packaging it with the app is a later distribution step and requires checking the FFmpeg build's license terms.

## Notes

The selected folder limits what the backend will inspect. Symbolic links are hidden. The app reads files but does not modify them. Video preview and editing are outside this first version.
