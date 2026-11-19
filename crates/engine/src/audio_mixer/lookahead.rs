use ffmpeg_next::frame;

/// Collect contiguous source audio without crossing a timestamp gap. A gap
/// contributes silence; overlapping samples are skipped, just as at output.
pub(crate) fn collect_audio_preview<'a>(
    samples: &mut Vec<[f32; 2]>,
    frames: impl Iterator<Item = &'a frame::Audio>,
    mut expected_pts: i64,
    limit: usize,
    jitter: u64,
) {
    samples.clear();
    samples.reserve(limit);

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

        let count = (frame.samples() - skip).min(limit - samples.len());

        if count > 0 {
            let left = &frame.plane::<f32>(0)[skip..skip + count];
            let right = &frame.plane::<f32>(1)[skip..skip + count];

            samples.extend(left.iter().zip(right).map(|(&left, &right)| [left, right]));
            expected_pts += count as i64;
        }

        if samples.len() == limit {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{
        ChannelLayout,
        format::sample::{Sample, Type},
    };

    use super::*;

    #[test]
    fn reused_preview_clears_previous_samples_and_preserves_gaps_and_overlaps() {
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 4, ChannelLayout::STEREO);

        audio.set_pts(Some(10));
        audio
            .plane_mut::<f32>(0)
            .copy_from_slice(&[1.0, 2.0, 3.0, 4.0]);
        audio
            .plane_mut::<f32>(1)
            .copy_from_slice(&[5.0, 6.0, 7.0, 8.0]);

        let mut preview = Vec::new();

        collect_audio_preview(&mut preview, [&audio].into_iter(), 8, 5, 0);
        assert_eq!(
            preview,
            [[0.0; 2], [0.0; 2], [1.0, 5.0], [2.0, 6.0], [3.0, 7.0]]
        );

        let storage = preview.as_ptr();

        collect_audio_preview(&mut preview, [&audio].into_iter(), 12, 5, 0);
        assert_eq!(preview, [[3.0, 7.0], [4.0, 8.0]]);
        assert_eq!(preview.as_ptr(), storage);
    }
}
