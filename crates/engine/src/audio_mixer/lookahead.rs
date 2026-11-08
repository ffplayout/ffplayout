use ffmpeg_next::frame;

/// Collect contiguous source audio without crossing a timestamp gap. A gap
/// contributes silence; overlapping samples are skipped, just as at output.
pub(crate) fn collect_audio_preview<'a>(
    frames: impl Iterator<Item = &'a frame::Audio>,
    mut expected_pts: i64,
    limit: usize,
    jitter: u64,
) -> Vec<[f32; 2]> {
    let mut samples = Vec::with_capacity(limit);

    for frame in frames {
        if frame.planes() != 2 {
            break;
        }

        let pts = frame.pts().unwrap_or(expected_pts);
        let difference = pts.saturating_sub(expected_pts);
        let skip = if pts.abs_diff(expected_pts) <= jitter {
            0
        } else if difference > 0 {
            let gap = (difference as usize).min(limit - samples.len());
            samples.resize(samples.len() + gap, [0.0; 2]);
            expected_pts += gap as i64;
            0
        } else {
            (difference.unsigned_abs() as usize).min(frame.samples())
        };

        for index in skip..frame.samples() {
            if samples.len() == limit {
                break;
            }

            samples.push([frame.plane::<f32>(0)[index], frame.plane::<f32>(1)[index]]);
            expected_pts += 1;
        }

        if samples.len() == limit {
            break;
        }
    }

    samples
}
