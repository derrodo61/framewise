use serde::Serialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::UNIX_EPOCH,
};
use tauri::{Emitter, Manager};

use crate::{AppState, run_ffprobe, video_file, within_root};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditSource {
    video_path: String,
    duration: f64,
    frame_rate: Option<f64>,
    frame_count: Option<u64>,
    has_audio: bool,
    source_signature: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditResult {
    output_path: String,
    backup_path: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EditProgress {
    source: String,
    percent: f64,
}

struct MediaInfo {
    duration: f64,
    frame_rate: Option<f64>,
    frame_count: Option<u64>,
    has_audio: bool,
    tags: BTreeMap<String, String>,
}

fn parse_frame_rate(rate: &str) -> Option<f64> {
    let value = match rate.split_once('/') {
        Some((numerator, denominator)) => {
            numerator.parse::<f64>().ok()? / denominator.parse::<f64>().ok()?
        }
        None => rate.parse::<f64>().ok()?,
    };
    (value.is_finite() && value > 0.0 && value <= 1000.0).then_some(value)
}

fn inspect(path: &Path) -> Result<MediaInfo, String> {
    let output = run_ffprobe(path).map_err(|error| format!("Could not start ffprobe: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not inspect video: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Invalid ffprobe output: {error}"))?;
    let streams = value["streams"]
        .as_array()
        .ok_or("The video has no streams")?;
    let video_count = streams
        .iter()
        .filter(|stream| stream["codec_type"] == "video")
        .count();
    let audio_count = streams
        .iter()
        .filter(|stream| stream["codec_type"] == "audio")
        .count();
    let video_stream = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video");
    if video_count != 1 || audio_count > 1 || streams.len() != video_count + audio_count {
        return Err("This editor supports one video track and up to one audio track, without subtitles or extra tracks.".into());
    }
    if value["chapters"]
        .as_array()
        .is_some_and(|chapters| !chapters.is_empty())
    {
        return Err("Editing videos with chapters is not supported yet.".into());
    }
    let duration = value["format"]["duration"]
        .as_str()
        .or_else(|| video_stream.and_then(|stream| stream["duration"].as_str()))
        .and_then(|text| text.parse::<f64>().ok())
        .ok_or("The video duration is unavailable")?;
    if !duration.is_finite() || duration <= 0.2 {
        return Err("The video is too short to edit".into());
    }
    let tags = value["format"]["tags"]
        .as_object()
        .map(|tags| {
            tags.iter()
                .filter_map(|(key, value)| {
                    value.as_str().map(|value| (key.clone(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(MediaInfo {
        duration,
        frame_rate: video_stream.and_then(|stream| {
            stream["avg_frame_rate"]
                .as_str()
                .and_then(parse_frame_rate)
                .or_else(|| stream["r_frame_rate"].as_str().and_then(parse_frame_rate))
        }),
        frame_count: video_stream
            .and_then(|stream| stream["nb_frames"].as_str())
            .and_then(|count| count.parse::<u64>().ok())
            .filter(|count| *count > 0),
        has_audio: audio_count == 1,
        tags,
    })
}

fn signature(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("Cannot read video: {error}"))?;
    let modified = metadata
        .modified()
        .map_err(|error| format!("Cannot read video modification time: {error}"))?
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("Invalid video modification time: {error}"))?;
    Ok(format!("{}:{}", metadata.len(), modified.as_nanos()))
}

#[tauri::command]
pub(crate) fn prepare_edit(
    path: String,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<EditSource, String> {
    let source = within_root(&path, &state)?;
    if !source.is_file() || !video_file(&source) {
        return Err("Select a supported video file".into());
    }
    let info = inspect(&source)?;
    app.asset_protocol_scope()
        .allow_file(&source)
        .map_err(|error| format!("Cannot open editor preview: {error}"))?;
    Ok(EditSource {
        video_path: source.to_string_lossy().into_owned(),
        duration: info.duration,
        frame_rate: info.frame_rate,
        frame_count: info.frame_count,
        has_audio: info.has_audio,
        source_signature: signature(&source)?,
    })
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

fn needs_metadata_keys(tags: &BTreeMap<String, String>) -> bool {
    tags.keys().any(|key| {
        ![
            "major_brand",
            "minor_version",
            "compatible_brands",
            "creation_time",
            "title",
            "comment",
            "encoder",
            "duration",
            "bit_rate",
            "number_of_frames",
        ]
        .iter()
        .any(|standard| key.eq_ignore_ascii_case(standard))
    })
}

fn preserved_tag(actual: Option<&String>, expected: &str, key: &str) -> bool {
    actual.is_some_and(|value| {
        value == expected
            || (key.eq_ignore_ascii_case("creation_time")
                && value.split(';').count() > 1
                && value.split(';').all(|part| part == expected))
    })
}

fn run_render(
    source: &Path,
    output: &Path,
    start: f64,
    end: f64,
    info: &MediaInfo,
    app: &tauri::AppHandle,
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
        let movflags = if needs_metadata_keys(&info.tags) {
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
        let source_label = source.to_string_lossy().into_owned();
        let _ = app.emit(
            "edit-progress",
            EditProgress {
                source: source_label.clone(),
                percent: 0.0,
            },
        );
        let mut details = String::new();
        if let Some(stderr) = child.stderr.take() {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(microseconds) = line
                    .strip_prefix("out_time_us=")
                    .and_then(|value| value.parse::<f64>().ok())
                {
                    let percent = (microseconds / (expected * 10_000.0)).clamp(0.0, 99.0);
                    let _ = app.emit(
                        "edit-progress",
                        EditProgress {
                            source: source_label.clone(),
                            percent,
                        },
                    );
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
        let _ = app.emit(
            "edit-progress",
            EditProgress {
                source: source_label,
                percent: 100.0,
            },
        );
        return Ok(());
    }
    Err(format!(
        "FFmpeg was not found. Install FFmpeg and add it to PATH, or set FFMPEG_PATH. {}",
        last_error.map_or(String::new(), |error| error.to_string())
    ))
}

fn verify_output(path: &Path, expected: f64, source: &MediaInfo) -> Result<(), String> {
    if fs::metadata(path)
        .map_err(|error| format!("Cannot read rendered video: {error}"))?
        .len()
        == 0
    {
        return Err("FFmpeg created an empty video".into());
    }
    let result = inspect(path)?;
    if (result.duration - expected).abs() > 1.0 {
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
        if !preserved_tag(result.tags.get(key), value, key) {
            return Err(format!(
                "The output could not preserve the metadata tag “{key}”. The original was not changed."
            ));
        }
    }
    Ok(())
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
    if let Err(error) = trash::delete(&backup) {
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
        run_render(&source, &temp, start, end, &info, &app)?;
        verify_output(&temp, info.duration - (end - start), &info)?;
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
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[tauri::command]
pub(crate) async fn export_edit(
    path: String,
    destination: String,
    source_signature: String,
    start: f64,
    end: f64,
    replace: bool,
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<EditResult, String> {
    let source = within_root(&path, &state)?;
    if !source.is_file() || !video_file(&source) {
        return Err("Select a supported video file".into());
    }
    let destination = output_path(&destination, &source, replace)?;
    tauri::async_runtime::spawn_blocking(move || {
        export_impl(
            source,
            destination,
            source_signature,
            start,
            end,
            replace,
            app,
        )
    })
    .await
    .map_err(|error| format!("Editor task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::{filter_graph, needs_metadata_keys, parse_frame_rate, preserved_tag, validate_cut};
    use std::collections::BTreeMap;

    #[test]
    fn frame_rate_accepts_video_ratios_and_rejects_missing_rates() {
        assert!((parse_frame_rate("30000/1001").unwrap() - 29.970_029_97).abs() < 0.000_01);
        assert_eq!(parse_frame_rate("24/1"), Some(24.0));
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("N/A"), None);
    }

    #[test]
    fn cut_must_leave_playable_video() {
        assert!(validate_cut(10.0, 2.0, 4.0).is_ok());
        assert!(validate_cut(10.0, 0.0, 9.9).is_err());
        assert!(validate_cut(10.0, 4.0, 4.0).is_err());
        assert!(validate_cut(10.0, -1.0, 2.0).is_err());
        assert!(validate_cut(10.0, f64::NAN, 2.0).is_err());
    }

    #[test]
    fn filters_keep_both_sides_of_a_middle_cut() {
        let graph = filter_graph(2.0, 4.0, 10.0, true);
        assert!(graph.contains("[va][vb]concat=n=2:v=1:a=0[vout]"));
        assert!(graph.contains("[aa][ab]concat=n=2:v=0:a=1[aout]"));
        assert!(!filter_graph(0.0, 2.0, 10.0, false).contains("[aout]"));
    }

    #[test]
    fn standard_mp4_tags_do_not_need_generic_metadata_keys() {
        let mut tags = BTreeMap::from([
            ("creation_time".into(), "2026-10-04T18:55:09.000000Z".into()),
            ("title".into(), "Recording".into()),
            ("comment".into(), "Captured with Snagit".into()),
        ]);
        assert!(!needs_metadata_keys(&tags));
        tags.insert("camera_model".into(), "Example".into());
        assert!(needs_metadata_keys(&tags));
    }

    #[test]
    fn duplicate_mp4_creation_time_values_still_preserve_the_date() {
        let expected = "2026-10-04T18:55:09.000000Z";
        assert!(preserved_tag(Some(&expected.to_string()), expected, "creation_time"));
        assert!(preserved_tag(
            Some(&format!("{expected};{expected}")),
            expected,
            "creation_time"
        ));
        assert!(!preserved_tag(
            Some(&format!("{expected};2026-10-05T18:55:09.000000Z")),
            expected,
            "creation_time"
        ));
    }
}
