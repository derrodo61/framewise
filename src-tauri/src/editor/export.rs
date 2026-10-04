use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::UNIX_EPOCH,
};
use tauri::Emitter;

use super::{MediaInfo, inspect, metadata, signature};
use crate::{AppState, video_file, within_root};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditResult {
    output_path: String,
    backup_path: Option<String>,
    metadata_warnings: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportRequest {
    path: String,
    destination: String,
    source_signature: String,
    start: f64,
    end: f64,
    replace: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EditProgress {
    source: String,
    percent: f64,
}

fn output_path(path: &str, source: &Path, replace: bool) -> Result<PathBuf, String> {
    if replace {
        if !source
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
        {
            return Err("Save can replace MP4 files only. Use Save As to create an MP4.".into());
        }
        return Ok(source.to_path_buf());
    }
    let requested = Path::new(path);
    if !requested.is_absolute()
        || !requested
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
    {
        return Err("Choose an absolute path ending in .mp4".into());
    }
    let name = requested.file_name().ok_or("Choose an MP4 file name")?;
    let parent = requested
        .parent()
        .ok_or("Choose an output folder")?
        .canonicalize()
        .map_err(|error| format!("Cannot open output folder: {error}"))?;
    let destination = parent.join(name);
    if destination.exists() {
        return Err("That file already exists. Choose a new name for Save As.".into());
    }
    Ok(destination)
}

fn temporary_path(parent: &Path, label: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos());
    for attempt in 0..1000 {
        let path = parent.join(format!(
            ".framewise-{label}-{}-{stamp}-{attempt}.mp4",
            std::process::id()
        ));
        if !path.exists() {
            return path;
        }
    }
    parent.join(format!(".framewise-{label}-{}.mp4", std::process::id()))
}

fn filter_graph(start: f64, end: f64, duration: f64, has_audio: bool) -> String {
    let segment = |kind: &str, input: &str, reset: &str, split: &str, trim: &str, output: &str| {
        if start <= 0.000_001 {
            format!("[{input}]{reset},{trim}=start={end:.6},{reset}[{output}]")
        } else if end >= duration - 0.000_001 {
            format!("[{input}]{reset},{trim}=end={start:.6},{reset}[{output}]")
        } else {
            format!(
                "[{input}]{reset},{split}=2[{kind}0][{kind}1];[{kind}0]{trim}=end={start:.6},{reset}[{kind}a];[{kind}1]{trim}=start={end:.6},{reset}[{kind}b];[{kind}a][{kind}b]concat=n=2:v={}:a={}[{output}]",
                if kind == "v" { 1 } else { 0 },
                if kind == "a" { 1 } else { 0 }
            )
        }
    };
    let video = segment("v", "0:v:0", "setpts=PTS-STARTPTS", "split", "trim", "vout");
    if has_audio {
        format!(
            "{video};{}",
            segment(
                "a",
                "0:a:0",
                "asetpts=PTS-STARTPTS",
                "asplit",
                "atrim",
                "aout"
            )
        )
    } else {
        video
    }
}

fn validate_cut(duration: f64, start: f64, end: f64) -> Result<(), String> {
    if !start.is_finite()
        || !end.is_finite()
        || start < 0.0
        || end <= start
        || end > duration + 0.01
        || duration - (end - start) < 0.2
    {
        return Err("Choose a valid section and leave at least 0.2 seconds of video.".into());
    }
    Ok(())
}

fn ffmpeg_candidates() -> Vec<PathBuf> {
    if let Some(configured) = std::env::var_os("FFMPEG_PATH") {
        return vec![PathBuf::from(configured)];
    }
    #[allow(unused_mut)]
    let mut candidates = vec![PathBuf::from("ffmpeg")];
    #[cfg(windows)]
    {
        let sibling = std::env::var_os("FFPROBE_PATH")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .map(|path| path.with_file_name("ffmpeg.exe"));
        let fallback = sibling
            .filter(|path| path.is_file())
            .or_else(|| crate::winget_ffprobe().map(|path| path.with_file_name("ffmpeg.exe")));
        if let Some(path) = fallback.filter(|path| path.is_file()) {
            candidates.push(path);
        }
    }
    candidates
}

fn run_render(
    source: &Path,
    output: &Path,
    start: f64,
    end: f64,
    info: &MediaInfo,
    mut on_progress: impl FnMut(f64),
) -> Result<(), String> {
    let expected = info.duration - (end - start);
    let mut last_error = None;
    for program in ffmpeg_candidates() {
        let mut command = Command::new(program);
        command
            .args(["-hide_banner", "-nostdin", "-loglevel", "error", "-i"])
            .arg(source)
            .args([
                "-filter_complex",
                &filter_graph(start, end, info.duration, info.has_audio),
                "-map",
                "[vout]",
            ]);
        if info.has_audio {
            command.args(["-map", "[aout]"]);
        }
        command.args(["-map_metadata", "0", "-map_metadata:s:v:0", "0:s:v:0"]);
        if info.has_audio {
            command.args(["-map_metadata:s:a:0", "0:s:a:0"]);
        }
        command.args([
            "-metadata",
            "major_brand=",
            "-metadata",
            "minor_version=",
            "-metadata",
            "compatible_brands=",
        ]);
        command.args([
            "-map_chapters",
            "-1",
            "-c:v",
            "libx264",
            "-preset",
            "fast",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
        ]);
        if info.has_audio {
            command.args(["-c:a", "aac", "-b:a", "192k"]);
        }
        let movflags = if metadata::needs_metadata_keys(&info.tags) {
            "+faststart+use_metadata_tags"
        } else {
            "+faststart"
        };
        command
            .args([
                "-movflags",
                movflags,
                "-progress",
                "pipe:2",
                "-nostats",
                "-y",
            ])
            .arg(output)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                last_error = Some(error);
                continue;
            }
            Err(error) => return Err(format!("Cannot start FFmpeg: {error}")),
        };
        on_progress(0.0);
        let mut details = String::new();
        if let Some(stderr) = child.stderr.take() {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(microseconds) = line
                    .strip_prefix("out_time_us=")
                    .and_then(|value| value.parse::<f64>().ok())
                {
                    let percent = (microseconds / (expected * 10_000.0)).clamp(0.0, 99.0);
                    on_progress(percent);
                } else if !line.contains('=') {
                    if details.len() > 3000 {
                        details.drain(..details.len() - 2000);
                    }
                    details.push_str(&line);
                    details.push('\n');
                }
            }
        }
        let status = child
            .wait()
            .map_err(|error| format!("Cannot wait for FFmpeg: {error}"))?;
        if !status.success() {
            return Err(format!(
                "FFmpeg could not render this video: {}",
                details.trim()
            ));
        }
        on_progress(100.0);
        return Ok(());
    }
    Err(format!(
        "FFmpeg was not found. Install FFmpeg and add it to PATH, or set FFMPEG_PATH. {}",
        last_error.map_or(String::new(), |error| error.to_string())
    ))
}

fn duration_tolerance(expected: f64) -> f64 {
    (expected * 0.05).clamp(0.05, 0.25)
}

fn verify_output(path: &Path, expected: f64, source: &MediaInfo) -> Result<Vec<String>, String> {
    if fs::metadata(path)
        .map_err(|error| format!("Cannot read rendered video: {error}"))?
        .len()
        == 0
    {
        return Err("FFmpeg created an empty video".into());
    }
    let result = inspect(path)?;
    if result.video_frames == Some(0) {
        return Err("Rendered video contains no frames".into());
    }
    if source.has_audio && !result.has_audio {
        return Err("Rendered video lost its audio track".into());
    }
    if (result.duration - expected).abs() > duration_tolerance(expected) {
        return Err(format!(
            "Rendered duration differs from the expected duration (expected {expected:.2}s, got {:.2}s)",
            result.duration
        ));
    }
    for (key, value) in &source.tags {
        if [
            "major_brand",
            "minor_version",
            "compatible_brands",
            "encoder",
            "duration",
            "bit_rate",
            "number_of_frames",
        ]
        .iter()
        .any(|technical| key.eq_ignore_ascii_case(technical))
        {
            continue;
        }
        if !metadata::preserved_tag(result.tags.get(key), value, key) {
            return Err(format!(
                "The output could not preserve the metadata tag “{key}”. The original was not changed."
            ));
        }
    }
    let mut warnings = Vec::new();
    if let Some(warning) =
        metadata::changed_stream_tags("Video", &source.video_tags, &result.video_tags)
    {
        warnings.push(warning);
    }
    if source.has_audio
        && let Some(warning) =
            metadata::changed_stream_tags("Audio", &source.audio_tags, &result.audio_tags)
    {
        warnings.push(warning);
    }
    Ok(warnings)
}

fn save_new_file(temp: &Path, destination: &Path) -> Result<(), String> {
    match fs::hard_link(temp, destination) {
        Ok(()) => {
            if let Err(error) = fs::remove_file(temp) {
                log::warn!("Could not remove temporary render: {error}");
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Err("That file already exists. Choose a new name for Save As.".into())
        }
        Err(_) => {
            let mut input = fs::File::open(temp)
                .map_err(|error| format!("Cannot open temporary render: {error}"))?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|error| format!("Cannot create output: {error}"))?;
            let result = io::copy(&mut input, &mut output)
                .and_then(|_| output.flush())
                .and_then(|_| output.sync_all());
            if let Err(error) = result {
                drop(output);
                let _ = fs::remove_file(destination);
                return Err(format!("Could not finish output: {error}"));
            }
            if let Err(error) = fs::remove_file(temp) {
                log::warn!("Could not remove temporary render: {error}");
            }
            Ok(())
        }
    }
}

fn replace_original(temp: &Path, source: &Path) -> Result<Option<String>, String> {
    replace_original_with(temp, source, |backup| trash::delete(backup))
}

fn replace_original_with<E: std::fmt::Display>(
    temp: &Path,
    source: &Path,
    dispose_backup: impl FnOnce(&Path) -> Result<(), E>,
) -> Result<Option<String>, String> {
    let parent = source.parent().ok_or("Cannot find the source folder")?;
    let name = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("video");
    let backup = temporary_path(parent, &format!("{name}-before-edit"));
    fs::rename(source, &backup)
        .map_err(|error| format!("Could not back up the original: {error}"))?;
    if let Err(error) = fs::rename(temp, source) {
        let restore = fs::rename(&backup, source);
        return Err(match restore {
            Ok(()) => format!("Could not replace the original; it was restored: {error}"),
            Err(restore_error) => format!(
                "Could not replace the original: {error}. The backup is at {} and could not be restored: {restore_error}",
                backup.display()
            ),
        });
    }
    if let Err(error) = dispose_backup(&backup) {
        log::warn!("Original backup could not be moved to Trash: {error}");
        return Ok(Some(backup.to_string_lossy().into_owned()));
    }
    Ok(None)
}

fn export_impl(
    source: PathBuf,
    destination: PathBuf,
    expected_signature: String,
    start: f64,
    end: f64,
    replace: bool,
    app: tauri::AppHandle,
) -> Result<EditResult, String> {
    if signature(&source)? != expected_signature {
        return Err(
            "The source video changed since you opened the editor. Reopen it before saving.".into(),
        );
    }
    let info = inspect(&source)?;
    validate_cut(info.duration, start, end)?;
    let parent = destination
        .parent()
        .ok_or("Cannot find the output folder")?;
    let temp = temporary_path(parent, "render");
    let result = (|| {
        let source_label = source.to_string_lossy().into_owned();
        run_render(&source, &temp, start, end, &info, |percent| {
            let _ = app.emit(
                "edit-progress",
                EditProgress {
                    source: source_label.clone(),
                    percent,
                },
            );
        })?;
        let metadata_warnings = verify_output(&temp, info.duration - (end - start), &info)?;
        if signature(&source)? != expected_signature {
            return Err("The source video changed while rendering. No file was replaced.".into());
        }
        let backup_path = if replace {
            replace_original(&temp, &source)?
        } else {
            save_new_file(&temp, &destination)?;
            None
        };
        Ok(EditResult {
            output_path: destination.to_string_lossy().into_owned(),
            backup_path,
            metadata_warnings,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[tauri::command]
pub(crate) async fn export_edit(
    request: ExportRequest,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<EditResult, String> {
    let source = within_root(&request.path, &state)?;
    if !source.is_file() || !video_file(&source) {
        return Err("Select a supported video file".into());
    }
    let destination = output_path(&request.destination, &source, request.replace)?;
    tauri::async_runtime::spawn_blocking(move || {
        export_impl(
            source,
            destination,
            request.source_signature,
            request.start,
            request.end,
            request.replace,
            app,
        )
    })
    .await
    .map_err(|error| format!("Editor task failed: {error}"))?
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
