use std::path::Path;

use crate::run_ffprobe_with_args;

pub(super) fn read(path: &Path) -> Result<Vec<f64>, String> {
    // Packet presentation timestamps are fast to read and preserve variable frame spacing.
    let output = run_ffprobe_with_args(
        path,
        &[
            "-select_streams",
            "v:0",
            "-show_entries",
            "packet=pts_time",
            "-of",
            "csv=p=0",
        ],
    )
    .map_err(|error| format!("Could not read video frame times: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "Could not read video frame times: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse(&output.stdout)
}

fn parse(stdout: &[u8]) -> Result<Vec<f64>, String> {
    let mut times: Vec<f64> = String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| line.split(',').next()?.trim().parse::<f64>().ok())
        .filter(|time| time.is_finite())
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup_by(|next, previous| (*next - *previous).abs() < 0.000_001);
    let first = *times.first().ok_or("The video has no frame timestamps")?;
    for time in &mut times {
        *time = (*time - first).max(0.0);
    }
    Ok(times)
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn packet_times_keep_variable_frame_spacing_in_presentation_order() {
        let times = parse(b"0.442967\n0.000000\n0.242400\n0.242400\nN/A\n").unwrap();
        assert_eq!(times, [0.0, 0.2424, 0.442967]);
    }

    #[test]
    fn packet_times_normalize_nonzero_stream_starts() {
        let times = parse(b"2.250000\n2.000000\n").unwrap();
        assert_eq!(times, [0.0, 0.25]);
        assert!(parse(b"N/A\n").is_err());
    }
}
