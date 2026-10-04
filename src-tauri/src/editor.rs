use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, fs, path::Path, time::UNIX_EPOCH};
use tauri::Manager;

use crate::{AppState, run_ffprobe, video_file, within_root};
pub(crate) mod export;
mod frame_times;
mod metadata;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EditSource {
    video_path: String,
    duration: f64,
    source_signature: String,
}

struct MediaInfo {
    duration: f64,
    video_frames: Option<u64>,
    has_audio: bool,
    tags: BTreeMap<String, String>,
    video_tags: BTreeMap<String, String>,
    audio_tags: BTreeMap<String, String>,
}

fn media_duration(value: &Value, streams: &[Value]) -> Option<f64> {
    std::iter::once(&value["format"])
        .chain(streams.iter())
        .filter_map(|item| item["duration"].as_str())
        .filter_map(|text| text.parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .max_by(f64::total_cmp)
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
    let duration = media_duration(&value, streams).ok_or("The video duration is unavailable")?;
    if !duration.is_finite() || duration <= 0.2 {
        return Err("The video is too short to edit".into());
    }
    let tags = metadata::tags(&value["format"]);
    let video_tags = video_stream.map(metadata::tags).unwrap_or_default();
    let audio_tags = streams
        .iter()
        .find(|stream| stream["codec_type"] == "audio")
        .map(metadata::tags)
        .unwrap_or_default();
    Ok(MediaInfo {
        duration,
        video_frames: video_stream
            .and_then(|stream| stream["nb_frames"].as_str())
            .and_then(|count| count.parse::<u64>().ok()),
        has_audio: audio_count == 1,
        tags,
        video_tags,
        audio_tags,
    })
}

#[cfg(test)]
mod duration_tests {
    use super::media_duration;
    use serde_json::json;

    #[test]
    fn uses_longest_reported_track_when_container_duration_is_shorter() {
        let probe = json!({
            "format": {"duration": "19.26"},
            "streams": [
                {"codec_type": "video", "duration": "30.00"},
                {"codec_type": "audio", "duration": "29.98"}
            ]
        });
        assert_eq!(media_duration(&probe, probe["streams"].as_array().unwrap()), Some(30.0));
    }
}

pub(crate) fn signature(path: &Path) -> Result<String, String> {
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
        source_signature: signature(&source)?,
    })
}

#[tauri::command]
pub(crate) async fn video_frame_times(
    path: String,
    source_signature: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<f64>, String> {
    let source = within_root(&path, &state)?;
    if !source.is_file() || !video_file(&source) {
        return Err("Select a supported video file".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        if signature(&source)? != source_signature {
            return Err(
                "The source video changed. Reopen the editor to navigate its frames.".into(),
            );
        }
        let times = frame_times::read(&source)?;
        if signature(&source)? != source_signature {
            return Err("The source video changed while reading its frames.".into());
        }
        Ok(times)
    })
    .await
    .map_err(|error| format!("Frame timestamp task failed: {error}"))?
}
