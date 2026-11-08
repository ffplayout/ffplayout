//! Zero-copy bridge from normalized engine frames to codec-independent PCM.
use std::{mem::size_of, ptr, slice};

use anyhow::{Context, Result};
use ff_audio::buffer::StereoBlock;
use ffmpeg_next::{
    ChannelLayout, ffi,
    format::{Sample, sample::Type},
    frame,
};

use crate::utils::ffmpeg::make_audio_frame_writable;

/// Empty or unsupported frames are no-ops, as in the original processors.
/// Failure to detach shared storage is propagated before any DSP is applied.
pub(super) fn stereo(frame: &mut frame::Audio) -> Result<StereoBlock<'_>> {
    stereo_with(frame, make_audio_frame_writable)
}

fn stereo_with(
    frame: &mut frame::Audio,
    make_writable: impl FnOnce(&mut frame::Audio) -> Result<()>,
) -> Result<StereoBlock<'_>> {
    let Some(mut planes) = validated_planes(frame) else {
        return Ok(empty_stereo());
    };

    // SAFETY: frame owns a live AVFrame. Shared FFmpeg storage must become
    // exclusive before constructing simultaneous mutable channel slices.
    if unsafe { ffi::av_frame_is_writable(frame.as_mut_ptr()) } == 0 {
        make_writable(frame)?;

        planes =
            validated_planes(frame).context("invalid audio storage after making frame writable")?;
    }

    // SAFETY: validated_planes checked alignment, allocation bounds and channel
    // non-overlap. Storage is writable/exclusive; frame remains exclusively
    // borrowed for the lifetime of the returned view.
    unsafe {
        let left = slice::from_raw_parts_mut(planes[0], frame.samples());
        let right = slice::from_raw_parts_mut(planes[1], frame.samples());

        StereoBlock::new(left, right)
    }
}

/// Read-only bridge used by meters; never accesses planes of an empty frame.
pub(crate) fn stereo_planes(frame: &frame::Audio) -> Option<(&[f32], &[f32])> {
    validated_planes(frame)?;

    Some((frame.plane::<f32>(0), frame.plane::<f32>(1)))
}

fn empty_stereo<'a>() -> StereoBlock<'a> {
    StereoBlock::new(&mut [], &mut []).expect("empty stereo block is valid")
}

fn validated_planes(frame: &frame::Audio) -> Option<[*mut f32; 2]> {
    if frame.samples() == 0
        || frame.format() != Sample::F32(Type::Planar)
        || frame.channel_layout() != ChannelLayout::STEREO
        || frame.planes() != 2
    {
        return None;
    }

    let bytes = frame.samples().checked_mul(size_of::<f32>())?;

    if bytes > isize::MAX as usize {
        return None;
    }

    // SAFETY: frame owns a live AVFrame. FFmpeg supplies extended_data for the
    // declared planes. No sample references are created until each non-null,
    // aligned pointer range is verified against its owning AVBufferRef.
    unsafe {
        let raw = &*frame.as_ptr();

        if raw.extended_data.is_null() || usize::try_from(raw.linesize[0]).ok()? < bytes {
            return None;
        }

        let mut planes = [ptr::null_mut(); 2];
        let mut ranges = [(0, 0); 2];

        for channel in 0..2 {
            let plane = (*raw.extended_data.add(channel)).cast::<f32>();

            if plane.is_null() || !plane.is_aligned() {
                return None;
            }

            let buffer = ffi::av_frame_get_plane_buffer(frame.as_ptr(), channel as i32);
            let buffer = buffer.as_ref()?;
            let start = plane as usize;
            let end = start.checked_add(bytes)?;
            let buffer_start = buffer.data as usize;
            let buffer_end = buffer_start.checked_add(buffer.size)?;

            if buffer.data.is_null() || start < buffer_start || end > buffer_end {
                return None;
            }

            planes[channel] = plane;
            ranges[channel] = (start, end);
        }

        if ranges[0].0 < ranges[1].1 && ranges[1].0 < ranges[0].1 {
            return None;
        }

        Some(planes)
    }
}

#[cfg(test)]
mod tests {
    use ff_audio::{AudioEffect, AudioEffectsControl, buffer::AudioBuffer, volume::GainEffect};

    use crate::{LiveDynamicsProcessor, LiveLoudnessConfig, LiveLoudnessProcessor};

    use super::*;

    #[test]
    fn processing_borrows_ffmpeg_planes_and_preserves_timing() {
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);

        audio.set_rate(48_000);
        audio.set_pts(Some(96_000));
        audio.plane_mut::<f32>(0).fill(0.5);
        audio.plane_mut::<f32>(1).fill(0.25);

        let left = audio.plane::<f32>(0).as_ptr();
        let right = audio.plane::<f32>(1).as_ptr();
        let mut gain = GainEffect::new(AudioEffectsControl::new(0.5).unwrap(), 48_000);

        {
            let mut block = stereo(&mut audio).unwrap();

            assert_eq!(block.plane(0).as_ptr(), left);
            assert_eq!(block.plane(1).as_ptr(), right);
            gain.process(&mut block);
        }

        assert_eq!(audio.pts(), Some(96_000));
        assert_eq!(audio.rate(), 48_000);
        assert_eq!(audio.samples(), 480);
        assert!(audio.plane::<f32>(0).iter().all(|&sample| sample == 0.25));
        assert!(audio.plane::<f32>(1).iter().all(|&sample| sample == 0.125));
    }

    #[test]
    fn empty_mono_packed_and_unallocated_frames_are_no_ops() {
        let mut metadata_only = frame::Audio::empty();

        metadata_only.set_format(Sample::F32(Type::Planar));
        metadata_only.set_channel_layout(ChannelLayout::STEREO);
        metadata_only.set_samples(480);

        let mut frames = [
            frame::Audio::empty(),
            frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::MONO),
            frame::Audio::new(Sample::F32(Type::Packed), 480, ChannelLayout::STEREO),
            metadata_only,
        ];
        let mut dynamics =
            LiveDynamicsProcessor::new(48_000, LiveLoudnessConfig::default()).unwrap();
        let mut loudness =
            LiveLoudnessProcessor::new(48_000, LiveLoudnessConfig::default()).unwrap();
        let before = dynamics.metrics();
        let mut level_meter = crate::analysis::audio_level::AudioLevelMeter::new(48_000, None);
        let meter_control = crate::LoudnessMeterControl::default();

        meter_control.subscribe();

        let mut loudness_meter =
            crate::analysis::loudness::LoudnessMeter::new(48_000, meter_control);

        for audio in &mut frames {
            let samples = audio.samples();

            assert!(stereo_planes(audio).is_none());
            assert_eq!(stereo(audio).unwrap().samples(), 0);
            dynamics.process(audio, &[]).unwrap();
            loudness.process(audio).unwrap();
            level_meter.process_frame(audio);
            loudness_meter.process_frame(audio);
            assert_eq!(audio.samples(), samples);
        }

        assert_eq!(dynamics.metrics(), before);
    }

    #[test]
    fn oversized_sample_counts_do_not_create_out_of_bounds_views() {
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);

        audio.set_samples(usize::MAX);
        assert!(stereo_planes(&audio).is_none());
        assert_eq!(stereo(&mut audio).unwrap().samples(), 0);
        audio.set_samples(480);
    }

    #[test]
    fn null_overlapping_unaligned_and_out_of_buffer_planes_are_rejected() {
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);

        // SAFETY: keep the AVFrame and its buffers alive, change only the plane
        // descriptor for validation, and restore it before dropping the frame.
        unsafe {
            let raw = audio.as_mut_ptr();
            let left = *(*raw).extended_data;
            let right = *(*raw).extended_data.add(1);
            let wrong_planes = [
                ptr::null_mut(),
                left,
                right.wrapping_add(1),
                right.wrapping_add(usize::MAX / 2),
            ];

            for wrong in wrong_planes {
                *(*raw).extended_data.add(1) = wrong;

                assert!(stereo_planes(&audio).is_none());
                assert_eq!(stereo(&mut audio).unwrap().samples(), 0);
            }

            *(*raw).extended_data.add(1) = right;
        }

        assert!(stereo_planes(&audio).is_some());
    }

    #[test]
    fn sample_bounds_are_checked_against_the_buffer_not_only_linesize() {
        let mut audio = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);
        let original_samples = audio.samples();

        // SAFETY: inflate metadata only; validation must not read sample storage.
        unsafe {
            let raw = audio.as_mut_ptr();
            let linesize = (*raw).linesize[0];

            (*raw).linesize[0] = 1_000_000;
            audio.set_samples(250_000);
            assert!(stereo_planes(&audio).is_none());
            assert_eq!(stereo(&mut audio).unwrap().samples(), 0);
            (*raw).linesize[0] = linesize;
        }

        audio.set_samples(original_samples);
        assert!(stereo_planes(&audio).is_some());
    }

    #[test]
    fn writable_failure_is_propagated_without_changing_samples() {
        let mut source = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);

        source.plane_mut::<f32>(0).fill(0.5);
        source.plane_mut::<f32>(1).fill(0.25);

        let mut referenced = crate::utils::ffmpeg::reference_audio_frame(&source).unwrap();
        let result = stereo_with(&mut referenced, |_| {
            Err(anyhow::anyhow!("simulated allocation failure"))
        });

        assert!(
            matches!(result, Err(error) if error.to_string() == "simulated allocation failure")
        );
        assert!(
            referenced
                .plane::<f32>(0)
                .iter()
                .all(|&sample| sample == 0.5)
        );
        assert!(source.plane::<f32>(0).iter().all(|&sample| sample == 0.5));
    }

    #[test]
    fn shared_frames_are_detached_before_mutable_samples_are_exposed() {
        let mut source = frame::Audio::new(Sample::F32(Type::Planar), 480, ChannelLayout::STEREO);

        source.plane_mut::<f32>(0).fill(0.5);
        source.plane_mut::<f32>(1).fill(0.25);

        let mut referenced = crate::utils::ffmpeg::reference_audio_frame(&source).unwrap();

        assert_eq!(
            source.plane::<f32>(0).as_ptr(),
            referenced.plane::<f32>(0).as_ptr()
        );
        stereo(&mut referenced).unwrap().plane_mut(0).fill(0.0);
        assert!(source.plane::<f32>(0).iter().all(|&sample| sample == 0.5));
        assert!(
            referenced
                .plane::<f32>(0)
                .iter()
                .all(|&sample| sample == 0.0)
        );
    }
}
