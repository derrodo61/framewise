use super::{
    duration_tolerance, ffmpeg_candidates, filter_graph, output_path, replace_original_with,
    run_render, save_new_file, validate_cut, verify_output,
};
use crate::editor::{frame_times, inspect, signature};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct TestFolder(PathBuf);

impl TestFolder {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "framewise-export-test-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestFolder {
    fn drop(&mut self) {
        if let Ok(files) = fs::read_dir(&self.0) {
            for file in files.flatten() {
                let _ = fs::remove_file(file.path());
            }
        }
        let _ = fs::remove_dir(&self.0);
    }
}

fn installed_ffmpeg() -> Option<PathBuf> {
    ffmpeg_candidates().into_iter().find(|program| {
        Command::new(program)
            .arg("-version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn fixture(ffmpeg: &Path, path: &Path, variable_rate: bool) {
    let mut command = Command::new(ffmpeg);
    command.args([
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "testsrc2=size=160x90:rate=30:duration=3",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=3",
    ]);
    if variable_rate {
        command.args(["-vf", r"select=lt(mod(n\,5)\,2)", "-fps_mode:v", "vfr"]);
    }
    command
        .args([
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-crf",
            "28",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "64k",
            "-metadata",
            "title=Framewise export test",
            "-metadata",
            "comment=Keep this tag",
            "-metadata",
            "creation_time=2026-01-02T03:04:05Z",
            "-y",
        ])
        .arg(path);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
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
fn duration_check_scales_with_short_and_long_exports() {
    assert_eq!(duration_tolerance(0.2), 0.05);
    assert_eq!(duration_tolerance(2.0), 0.1);
    assert_eq!(duration_tolerance(20.0), 0.25);
}

#[test]
fn exports_start_middle_and_end_cuts_with_audio_and_metadata() {
    let ffmpeg = installed_ffmpeg().expect("FFmpeg is required for editor export tests");
    let folder = TestFolder::new();
    let source = folder.0.join("source.mp4");
    fixture(&ffmpeg, &source, false);
    let info = inspect(&source).unwrap();
    let original_signature = signature(&source).unwrap();
    for (name, start, end) in [
        ("start", 0.0, 0.6),
        ("middle", 1.0, 1.6),
        ("end", 2.4, info.duration),
    ] {
        let output = folder.0.join(format!("{name}.mp4"));
        validate_cut(info.duration, start, end).unwrap();
        run_render(&source, &output, start, end, &info, |_| {}).unwrap();
        verify_output(&output, info.duration - (end - start), &info).unwrap();
        let rendered = inspect(&output).unwrap();
        assert!(rendered.has_audio, "{name} cut lost its audio track");
        assert!(
            rendered.video_frames.unwrap_or(0) > 0,
            "{name} cut lost its video frames"
        );
        assert_eq!(rendered.tags.get("title"), info.tags.get("title"));
        assert_eq!(rendered.tags.get("comment"), info.tags.get("comment"));
        assert_eq!(signature(&source).unwrap(), original_signature);
    }
}

#[test]
fn exports_variable_frame_rate_video_with_audio() {
    let ffmpeg = installed_ffmpeg().expect("FFmpeg is required for editor export tests");
    let folder = TestFolder::new();
    let source = folder.0.join("variable-rate.mp4");
    fixture(&ffmpeg, &source, true);
    let times = frame_times::read(&source).unwrap();
    let gaps: Vec<f64> = times.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(gaps.iter().any(|gap| *gap < 0.05));
    assert!(gaps.iter().any(|gap| *gap > 0.08));

    let info = inspect(&source).unwrap();
    let output = folder.0.join("variable-rate-edited.mp4");
    run_render(&source, &output, 1.0, 1.6, &info, |_| {}).unwrap();
    verify_output(&output, info.duration - 0.6, &info).unwrap();
    assert!(inspect(&output).unwrap().has_audio);
}

#[test]
fn save_as_creates_a_new_file_without_overwriting_an_existing_one() {
    let folder = TestFolder::new();
    let source = folder.0.join("source.mp4");
    let destination = folder.0.join("edited.mp4");
    let render = folder.0.join("render.mp4");
    fs::write(&source, b"original").unwrap();
    fs::write(&render, b"edited").unwrap();

    let selected = output_path(destination.to_str().unwrap(), &source, false).unwrap();
    save_new_file(&render, &selected).unwrap();
    assert_eq!(fs::read(&source).unwrap(), b"original");
    assert_eq!(fs::read(&destination).unwrap(), b"edited");
    assert!(!render.exists());

    fs::write(&render, b"another edit").unwrap();
    assert!(output_path(destination.to_str().unwrap(), &source, false).is_err());
    assert!(save_new_file(&render, &destination).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"edited");
}

#[test]
fn save_replaces_verified_output_and_retains_backup_when_trash_fails() {
    let folder = TestFolder::new();
    let source = folder.0.join("source.mp4");
    let render = folder.0.join("render.mp4");
    fs::write(&source, b"original").unwrap();
    fs::write(&render, b"edited").unwrap();

    let backup = replace_original_with(&render, &source, |_path| {
        Err::<(), _>(io::Error::other("Trash unavailable"))
    })
    .unwrap()
    .unwrap();
    assert_eq!(fs::read(&source).unwrap(), b"edited");
    assert_eq!(fs::read(&backup).unwrap(), b"original");
    assert!(!render.exists());
}

#[test]
fn save_replaces_original_and_disposes_its_backup() {
    let folder = TestFolder::new();
    let source = folder.0.join("source.mp4");
    let render = folder.0.join("render.mp4");
    fs::write(&source, b"original").unwrap();
    fs::write(&render, b"edited").unwrap();
    let old_version = signature(&source).unwrap();

    let backup = replace_original_with(&render, &source, |path| fs::remove_file(path)).unwrap();
    assert!(backup.is_none());
    assert_eq!(fs::read(&source).unwrap(), b"edited");
    assert_ne!(signature(&source).unwrap(), old_version);
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
}

#[test]
fn save_restores_original_if_replacement_fails() {
    let folder = TestFolder::new();
    let source = folder.0.join("source.mp4");
    let missing_render = folder.0.join("missing-render.mp4");
    fs::write(&source, b"original").unwrap();

    assert!(
        replace_original_with(&missing_render, &source, |_path| Ok::<(), io::Error>(())).is_err()
    );
    assert_eq!(fs::read(&source).unwrap(), b"original");
    assert_eq!(fs::read_dir(&folder.0).unwrap().count(), 1);
}
