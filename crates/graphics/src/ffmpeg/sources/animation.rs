//! Hybrid animation assets: fully cached short sources or tracked background decoding.
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use ffmpeg_next::{Dictionary, format};

use crate::{
    playback::Repeat,
    preparation::PreparationWeak,
    source::{Completion, FrameTiming, LoopDuration},
    style::LogoStyle,
    surface::{ColorEncoding, Resource, ResourceKey, SurfaceDescriptor},
};

use crate::ffmpeg::compositor::{
    metadata::yuva_descriptor, overlay::OverlayFrame, scene::CpuScene,
};

mod streaming;

use super::decoder::LogoDecoder;
use streaming::StreamSource;

#[derive(Clone, Copy)]
pub struct AnimationOptions {
    pub size: Option<(u32, u32)>,
    pub repeat: Repeat,
    pub completion: Completion,
}

pub(in crate::ffmpeg) struct PreparedSurface {
    frames: Vec<(FrameTiming, Arc<OverlayFrame>)>,
    playback: Option<(LoopDuration, Repeat, Completion)>,
    stream: Option<StreamSource>,
}

impl PreparedSurface {
    pub(in crate::ffmpeg) fn end_us(&self) -> Option<i64> {
        if let Some(stream) = &self.stream {
            return stream.end_us();
        }
        let (duration, repeat, completion) = self.playback?;
        if completion != Completion::Hide {
            return None;
        }

        duration.total_us(repeat)
    }

    pub(in crate::ffmpeg) fn still(surface: Arc<OverlayFrame>) -> Self {
        Self {
            frames: vec![(
                FrameTiming {
                    start_us: 0,
                    duration_us: 1,
                },
                surface,
            )],
            playback: None,
            stream: None,
        }
    }

    pub(in crate::ffmpeg) fn select(
        &self,
        elapsed_us: i64,
        instance: (u64, u64, i64),
    ) -> Option<Arc<OverlayFrame>> {
        if let Some(stream) = &self.stream {
            return stream.at(elapsed_us, instance);
        }

        self.at(elapsed_us).cloned()
    }

    pub(in crate::ffmpeg) fn poster(&self) -> Option<Arc<OverlayFrame>> {
        self.frames.first().map(|(_, surface)| surface.clone())
    }

    pub(in crate::ffmpeg) fn prewarm(&self, elapsed_us: i64, instance: (u64, u64, i64)) {
        if let Some(stream) = &self.stream {
            stream.prewarm(elapsed_us, instance);
        }
    }

    pub(in crate::ffmpeg) fn begin_frame(&self, epoch: i64) {
        if let Some(stream) = &self.stream {
            stream.begin_frame(epoch);
        }
    }

    pub(in crate::ffmpeg) fn end_frame(&self) {
        if let Some(stream) = &self.stream {
            stream.end_frame();
        }
    }

    pub(in crate::ffmpeg) fn suspend(&self) {
        if let Some(stream) = &self.stream {
            stream.suspend();
        }
    }

    pub(in crate::ffmpeg) fn stop(&self) {
        if let Some(stream) = &self.stream {
            stream.stop();
        }
    }

    pub(in crate::ffmpeg) fn at(&self, elapsed_us: i64) -> Option<&Arc<OverlayFrame>> {
        let Some((duration, repeat, completion)) = self.playback else {
            return Some(&self.frames[0].1);
        };
        let local = duration.playback_position(elapsed_us, repeat, completion)?;
        let index = self
            .frames
            .partition_point(|(timing, _)| timing.start_us <= local)
            .saturating_sub(1);

        Some(&self.frames[index].1)
    }
}

impl CpuScene {
    /// Call on a preparation worker after channel-storage containment checks.
    /// Only regular local files and non-nesting demuxers are accepted; decoded storage is bounded.
    /// Audio streams are never decoded.
    pub fn prepare_animation(
        &mut self,
        key: ResourceKey,
        path: &Path,
        options: AnimationOptions,
        cancelled: impl Fn() -> bool,
    ) -> Result<Arc<Resource>> {
        let started = Instant::now();
        let cancelled = || cancelled() || started.elapsed() > Duration::from_secs(10);
        ensure!(!cancelled(), "animation preparation cancelled");
        let metadata = path.metadata()?;
        ensure!(
            metadata.is_file(),
            "animation source must be a regular file"
        );
        let mut input_options = Dictionary::new();
        input_options.set("protocol_whitelist", "file");
        input_options.set("format_whitelist", "gif,mov,matroska,webm,avi,mpegts");
        let input = format::input_with_dictionary(path, input_options)?;
        let duration_hint = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .filter(|stream| stream.duration() > 0)
            .map(|stream| {
                LoopDuration::from_ticks(
                    stream.duration(),
                    stream.time_base().0,
                    stream.time_base().1,
                )
            })
            .or_else(|| {
                (input.duration() > 0)
                    .then(|| LoopDuration::from_ticks(input.duration(), 1, 1_000_000))
            });
        if let Some((width, height)) = options.size {
            ensure!(
                width > 0
                    && height > 0
                    && width <= 8192
                    && height <= 8192
                    && width.is_multiple_of(2)
                    && height.is_multiple_of(2),
                "animation size must be even and within 2..8192"
            );
        }
        let style = LogoStyle {
            scale: options
                .size
                .map(|(width, height)| format!("{width}:{height}")),
            opacity: 1.0,
            position: "0:0".into(),
        };
        let mut decoder = LogoDecoder::new(input, style.clone(), 2, 2)?;
        let mut frames: Vec<(FrameTiming, Arc<OverlayFrame>)> = Vec::new();
        let mut bytes = 0_usize;
        let mut descriptor: Option<SurfaceDescriptor> = None;
        let mut duration = LoopDuration::from_ticks(1, 1, 1_000_000);
        // Known long sources need only a poster before publication, rather than
        // decoding five seconds while a scheduled entrance is already elapsing.
        let stream_from_start = duration_hint
            .and_then(|duration| duration.total_us(Repeat::default()))
            .is_some_and(|end| end > 5_000_000);

        while let Some(decoded) = decoder.next_frame_with_cancel(cancelled)? {
            ensure!(!cancelled(), "animation preparation cancelled");
            // Switch to streaming instead of retaining an entire long source.
            ensure!(
                decoded.surface.storage_bytes() <= 64 * 1024 * 1024,
                "one animation frame exceeds 64 MiB; reduce size"
            );
            let reservation = self
                .budget
                .as_ref()
                .map(|budget| budget.reserve(decoded.surface.storage_bytes()))
                .transpose();
            if !frames.is_empty()
                && (stream_from_start
                    || frames.len() >= 256
                    || decoded.timing.end_us() > 5_000_000
                    || bytes.saturating_add(decoded.surface.storage_bytes()) > 64 * 1024 * 1024
                    || reservation.is_err())
            {
                drop(reservation);
                let poster_timing = frames
                    .first()
                    .map(|frame| frame.0)
                    .ok_or_else(|| anyhow::anyhow!("animation has no frames"))?;
                let first = frames
                    .first()
                    .ok_or_else(|| {
                        anyhow::anyhow!("one animation frame exceeds 64 MiB; reduce size")
                    })?
                    .1
                    .clone();
                drop(frames);
                drop(decoder);
                let mut descriptor =
                    descriptor.ok_or_else(|| anyhow::anyhow!("animation has no frames"))?;
                let service = self
                    .preparation
                    .as_ref()
                    .and_then(PreparationWeak::upgrade)
                    .ok_or_else(|| {
                        anyhow::anyhow!("streaming animation needs a channel preparation service")
                    })?;
                let stream = StreamSource::start(
                    path,
                    style,
                    options,
                    first.clone(),
                    poster_timing,
                    duration_hint,
                    &service,
                )?;
                // Dynamic pixels are accounted by their shared channel leases.
                descriptor.storage_bytes = first.frame.data(0).len()
                    + first.frame.data(1).len()
                    + first.frame.data(2).len()
                    + first.frame.data(3).len()
                    + first.chroma_alpha.len();
                return self.surfaces.insert(
                    key,
                    descriptor,
                    Arc::new(PreparedSurface {
                        frames: vec![(
                            FrameTiming {
                                start_us: 0,
                                duration_us: 1,
                            },
                            first,
                        )],
                        playback: None,
                        stream: Some(stream),
                    }),
                );
            }
            ensure!(
                decoded.timing.duration_us > 0
                    && frames
                        .last()
                        .is_none_or(|(timing, _)| decoded.timing.start_us > timing.start_us),
                "animation frame timestamps must advance"
            );
            let logo = decoded.surface;
            let size = logo.storage_bytes();
            bytes = bytes
                .checked_add(size)
                .ok_or_else(|| anyhow::anyhow!("animation byte count overflow"))?;
            ensure!(
                bytes <= 64 * 1024 * 1024,
                "animation exceeds 64 MiB prepared pixels"
            );
            let lease = reservation?;
            let frame = crate::ffmpeg::reference_video_frame(&logo.frame)?;
            let bounds = logo.as_overlay().bounds;
            let (width, height) = (frame.width(), frame.height());
            let alpha = logo.as_overlay().chroma_alpha.to_vec();
            let current =
                yuva_descriptor(&frame, bounds, alpha.len(), ColorEncoding::Bt601Limited)?;
            if let Some(first) = descriptor {
                ensure!(
                    (current.width, current.height) == (first.width, first.height),
                    "animation changes prepared dimensions"
                );
            } else {
                descriptor = Some(current);
            }
            duration = decoded.loop_end;
            frames.push((
                decoded.timing,
                Arc::new(OverlayFrame {
                    lease,
                    frame,
                    bounds,
                    chroma_alpha: alpha,
                    width,
                    height,
                    x: 0,
                    y: 0,
                    opacity: 255,
                }),
            ));
        }
        ensure!(!cancelled(), "animation preparation cancelled");
        let mut descriptor =
            descriptor.ok_or_else(|| anyhow::anyhow!("animation has no frames"))?;
        // The resource owns all cached frames, rather than only its poster frame.
        descriptor.storage_bytes = bytes;
        self.surfaces.insert(
            key,
            descriptor,
            Arc::new(PreparedSurface {
                frames,
                playback: Some((duration, options.repeat, options.completion)),
                stream: None,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroU32, path::PathBuf};

    use crate::{
        playback::{Presentation, Transition},
        preparation::ResourceBudget,
        scene::{Layer, LayerId, LayerSpec, SceneRevision, Scope},
    };
    use ffmpeg_next::{format::Pixel, frame};

    use super::*;

    fn path(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_images")
            .join(name)
    }

    fn options() -> AnimationOptions {
        AnimationOptions {
            size: Some((16, 16)),
            repeat: Repeat::Forever,
            completion: Completion::Hide,
        }
    }

    fn scene(resource: Arc<Resource>, start: i64) -> SceneRevision {
        SceneRevision {
            number: 1,
            layers: vec![Layer {
                spec: LayerSpec {
                    id: LayerId(1),
                    resource: resource.key,
                    z: 0,
                    scope: Scope::default(),
                    canvas: (32, 32),
                    position: (0, 0),
                    opacity: 1.0,
                },
                resource,
                presentation: Presentation::new(start, Transition::default()).unwrap(),
                visible: true,
            }]
            .into(),
        }
    }

    fn render(cpu: &CpuScene, scene: &SceneRevision, time: i64, size: u32) -> Vec<u8> {
        let mut target = frame::Video::new(Pixel::YUV420P, size, size);
        for plane in 0..3 {
            target
                .data_mut(plane)
                .fill(if plane == 0 { 16 } else { 128 });
        }
        let mut cache = crate::ffmpeg::scene::SceneScaleCache::default();
        cpu.blend_scaled(
            scene,
            time,
            Scope::default(),
            &mut target,
            ColorEncoding::Bt601Limited,
            &mut cache,
        );

        target.data(0).to_vec()
    }

    #[test]
    fn long_video_preparation_reads_only_the_initial_frames() {
        let service = crate::preparation::PreparationService::default();
        let mut cpu = CpuScene::new(1, 64 * 1024 * 1024).with_preparation(&service);
        let checks = std::cell::Cell::new(0);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo-video.mkv"),
                options(),
                || {
                    checks.set(checks.get() + 1);
                    checks.get() > 64
                },
            )
            .expect("known long videos must not decode the five-second cache window");
        assert!(cpu.surfaces.get(&resource).unwrap().stream.is_some());
        cpu.stop_streams();
        assert!(service.shutdown(Duration::from_secs(2)));
    }

    #[test]
    fn streaming_entrance_fades_the_poster_without_waiting_for_decoder_pixels() {
        use crate::ffmpeg::compositor::overlay::blend_overlay;

        let service = crate::preparation::PreparationService::default();
        let mut cpu = CpuScene::new(1, 64 * 1024 * 1024).with_preparation(&service);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo-video.mkv"),
                options(),
                || false,
            )
            .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap().clone();
        // No request has been made: stopping the decoder deterministically
        // models unavailable pixels without depending on thread scheduling.
        source.stream.as_ref().unwrap().stop();
        assert!(service.join(Duration::from_secs(2)));
        let mut scene = scene(resource, 0);
        Arc::make_mut(&mut scene.layers)[0].presentation = Presentation::new(
            0,
            Transition {
                fade_in_us: 200_000,
                ..Default::default()
            },
        )
        .unwrap();
        let background = || {
            let mut frame = frame::Video::new(Pixel::YUV420P, 32, 32);
            for plane in 0..3 {
                frame
                    .data_mut(plane)
                    .fill(if plane == 0 { 16 } else { 128 });
            }

            frame
        };
        let mut actual = background();
        let mut expected = background();
        let frozen = cpu.freeze(&scene, 100_000, Scope::default());
        assert!(!frozen.is_empty());
        frozen.blend(&mut actual, ColorEncoding::Bt601Limited);
        blend_overlay(
            &mut expected,
            source.poster().unwrap().as_ref().as_ref(),
            0.5,
        );
        for plane in 0..3 {
            assert_eq!(actual.data(plane), expected.data(plane));
        }

        assert!(
            cpu.freeze(&scene, 300_000, Scope::default()).is_empty(),
            "a later seek must not fall back to the first image"
        );
        assert!(service.shutdown(Duration::from_secs(2)));
    }

    #[test]
    fn finite_video_and_gif_fade_pixels_at_entrance_and_before_source_end() {
        use crate::ffmpeg::compositor::overlay::blend_overlay;
        use crate::preparation::PreparationService;

        for name in ["animated-logo.gif", "animated-logo-video.mkv"] {
            let service = PreparationService::default();
            let mut cpu = CpuScene::new(4, 64 * 1024 * 1024).with_preparation(&service);
            let resource = cpu
                .prepare_animation(
                    ResourceKey { id: 1, revision: 1 },
                    &path(name),
                    AnimationOptions {
                        repeat: Repeat::Count(NonZeroU32::MIN),
                        ..options()
                    },
                    || false,
                )
                .unwrap();
            let source = cpu.surfaces.get(&resource).unwrap().clone();
            let end = source
                .end_us()
                .expect("finite source duration is available before playback");
            let mut scene = scene(resource, 0);
            Arc::make_mut(&mut scene.layers)[0].presentation = Presentation::new(
                0,
                Transition {
                    fade_in_us: 200_000,
                    fade_out_us: 200_000,
                    ..Default::default()
                },
            )
            .unwrap();
            for (time, opacity) in [
                (0, 0.0),
                (100_000, 0.5),
                (200_000, 1.0),
                (end - 100_000, 0.5),
                (end, 0.0),
            ] {
                let surface = if opacity > 0.0 {
                    let deadline = Instant::now() + Duration::from_secs(5);
                    loop {
                        source.select(time, (0, 1, 0));
                        if source
                            .stream
                            .as_ref()
                            .is_none_or(|stream| stream.ready_at(time, (0, 1, 0)))
                        {
                            break;
                        }
                        assert!(Instant::now() < deadline);
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Some(
                        source
                            .select(time, (0, 1, 0))
                            .expect("visible source frame is ready"),
                    )
                } else {
                    None
                };
                let background = || {
                    let mut frame = frame::Video::new(Pixel::YUV420P, 32, 32);
                    for plane in 0..3 {
                        frame
                            .data_mut(plane)
                            .fill(if plane == 0 { 16 } else { 128 });
                    }
                    frame
                };
                let mut actual = background();
                let mut expected = background();
                cpu.blend(
                    &scene,
                    time,
                    Scope::default(),
                    &mut actual,
                    ColorEncoding::Bt601Limited,
                );
                if let Some(surface) = surface {
                    blend_overlay(&mut expected, surface.as_ref().as_ref(), opacity);
                }
                for plane in 0..3 {
                    assert_eq!(
                        actual.data(plane),
                        expected.data(plane),
                        "{name}, t={time}, plane={plane}"
                    );
                }
            }
            cpu.stop_streams();
            assert!(service.shutdown(Duration::from_secs(2)));
        }
    }

    #[test]
    fn short_finite_gif_reaches_full_opacity_with_proportionally_shortened_fades() {
        use crate::ffmpeg::compositor::overlay::blend_overlay;
        let mut cpu = CpuScene::new(1, 1024 * 1024);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo.gif"),
                AnimationOptions {
                    repeat: Repeat::default(),
                    ..options()
                },
                || false,
            )
            .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap();
        assert_eq!(source.end_us(), Some(600_000));
        let mut selected = scene(resource.clone(), 0);
        Arc::make_mut(&mut selected.layers)[0].presentation = Presentation::new(
            0,
            Transition {
                fade_in_us: 1_000_000,
                fade_out_us: 1_000_000,
                ..Default::default()
            }
            .within(5_000_000),
        )
        .unwrap();
        for (time, opacity) in [(150_000, 0.5), (300_000, 1.0), (450_000, 0.5)] {
            let mut actual = frame::Video::new(Pixel::YUV420P, 32, 32);
            let mut expected = frame::Video::new(Pixel::YUV420P, 32, 32);
            for plane in 0..3 {
                actual.data_mut(plane).fill(128);
                expected.data_mut(plane).fill(128);
            }
            cpu.blend(
                &selected,
                time,
                Scope::default(),
                &mut actual,
                ColorEncoding::Bt601Limited,
            );
            let source = source.select(time, (0, 1, 0)).unwrap();
            blend_overlay(&mut expected, source.as_ref().as_ref(), opacity);
            for plane in 0..3 {
                assert_eq!(actual.data(plane), expected.data(plane));
            }
        }
        assert!(cpu.freeze(&selected, 600_000, Scope::default()).is_empty());
    }

    #[test]
    fn gif_time_selection_seeks_repeats_retakes_and_scaled_outputs_use_ready_pixels() {
        let mut cpu = CpuScene::new(4, 1024 * 1024);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo.gif"),
                options(),
                || false,
            )
            .unwrap();
        let pixels = cpu.surfaces.get(&resource).unwrap();
        assert_eq!(pixels.frames.len(), 3);
        assert!(Arc::ptr_eq(
            pixels.at(0).unwrap(),
            pixels.at(600_000).unwrap()
        ));
        assert!(!Arc::ptr_eq(
            pixels.at(0).unwrap(),
            pixels.at(100_000).unwrap()
        ));
        for fps in [24, 25, 30, 50, 60] {
            for frame in 0..fps * 3 {
                let time = i64::from(frame) * 1_000_000 / i64::from(fps);
                let phase = time % 600_000;
                let expected = if phase < 100_000 {
                    0
                } else if phase < 400_000 {
                    1
                } else {
                    2
                };
                assert!(Arc::ptr_eq(
                    pixels.at(time).unwrap(),
                    &pixels.frames[expected].1
                ));
            }
        }
        let first = scene(resource.clone(), 0);
        let retake = scene(resource, 1_000_000);
        for size in [32, 16] {
            assert_eq!(
                render(&cpu, &first, 100_000, size),
                render(&cpu, &first, 700_000, size)
            );
            assert_eq!(
                render(&cpu, &first, 0, size),
                render(&cpu, &retake, 1_000_000, size)
            );
            let before_seek = render(&cpu, &first, 100_000, size);
            let _future = render(&cpu, &first, 9_900_000, size);
            assert_eq!(before_seek, render(&cpu, &first, 100_000, size));
        }
    }

    #[test]
    fn finite_animation_can_hide_or_hold_and_cached_leases_retire_after_queued_scenes() {
        let budget = ResourceBudget::new(1024 * 1024, 1024, 1);
        let mut cpu = CpuScene::new(4, 1024 * 1024).with_budget(budget.clone());
        for (id, completion) in [(1, Completion::Hide), (2, Completion::HoldLast)] {
            let resource = cpu
                .prepare_animation(
                    ResourceKey { id, revision: id },
                    &path("animated-logo.gif"),
                    AnimationOptions {
                        repeat: Repeat::Count(NonZeroU32::new(2).unwrap()),
                        completion,
                        ..options()
                    },
                    || false,
                )
                .unwrap();
            let pixels = cpu.surfaces.get(&resource).unwrap();
            assert!(pixels.at(1_199_999).is_some());
            match completion {
                Completion::Hide => assert!(pixels.at(1_200_000).is_none()),
                Completion::HoldLast => assert!(Arc::ptr_eq(
                    pixels.at(i64::MAX).unwrap(),
                    &pixels.frames.last().unwrap().1
                )),
            }
            let queued = scene(resource.clone(), 0);
            drop(resource);
            cpu.retire();
            assert!(budget.usage().0 > 0);
            drop(queued);
            cpu.retire();
            assert_eq!(budget.usage().0, 0);
        }
    }

    #[test]
    fn streaming_seek_loop_multiple_presentations_and_frozen_output_keep_their_pixels() {
        use crate::preparation::PreparationService;
        let service = PreparationService::default();
        let mut cpu = CpuScene::new(4, 64 * 1024 * 1024).with_preparation(&service);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo-video.mkv"),
                options(),
                || false,
            )
            .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap();
        assert!(source.stream.is_some());
        let input = format::input(&path("animated-logo-video.mkv")).unwrap();
        let mut reference = LogoDecoder::new(
            input,
            LogoStyle {
                scale: Some("16:16".into()),
                opacity: 1.0,
                position: "0:0".into(),
            },
            2,
            2,
        )
        .unwrap();
        let mut decoded = Vec::new();
        while let Some(frame) = reference.next_frame().unwrap() {
            decoded.push(frame);
        }
        let duration = decoded.last().unwrap().loop_end;
        let wait = |elapsed, instance| {
            let local = duration.position_us(elapsed);
            let expected = decoded
                .iter()
                .find(|frame| frame.timing.contains(local))
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(actual) = source.select(elapsed, instance)
                    && source.stream.as_ref().unwrap().ready_at(elapsed, instance)
                    && (0..4).all(|plane| {
                        (0..actual.frame.plane_height(plane) as usize).all(|row| {
                            let width = actual.frame.plane_width(plane) as usize;
                            actual.frame.data(plane)[row * actual.frame.stride(plane)
                                ..row * actual.frame.stride(plane) + width]
                                == expected.surface.frame.data(plane)[row
                                    * expected.surface.frame.stride(plane)
                                    ..row * expected.surface.frame.stride(plane) + width]
                        })
                    })
                {
                    return;
                }
                assert!(Instant::now() < deadline, "stream did not reach {elapsed}");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        for elapsed in [0, 2_100_000, 6_100_000, 1_200_000, 10_100_000] {
            wait(elapsed, (0, 1, 0));
        }
        wait(200_000, (0, 2, 0));
        wait(3_100_000, (0, 1, 0));
        wait(200_000, (0, 2, 0));
        wait(0, (0, 1, 9_000_000));
        let stream = source.stream.as_ref().unwrap();
        stream.suspend();
        let deadline = Instant::now() + Duration::from_secs(2);
        while service.budget().usage().0 > resource.descriptor.storage_bytes {
            assert!(
                Instant::now() < deadline,
                "inactive source must release cached/ready pixels"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let paused_count = stream.stats().0;
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(stream.stats().0, paused_count);
        stream.begin_frame(0);
        wait(0, (0, 1, 9_000_000));
        let (decoded_before, buffered_bytes) = stream.stats();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(
            stream.stats().0,
            decoded_before,
            "idle source must not keep decoding"
        );
        assert!(buffered_bytes <= 64 * 1024 * 1024);
        let selected = scene(resource.clone(), 9_000_000);
        let frozen = cpu.freeze(&selected, 9_000_000, Scope::default());
        let render_frozen = || {
            let mut target = frame::Video::new(Pixel::YUV420P, 32, 32);
            for plane in 0..3 {
                target.data_mut(plane).fill(128);
            }
            frozen.blend(&mut target, ColorEncoding::Bt601Limited);

            target.data(0).to_vec()
        };
        let before = render_frozen();
        wait(4_100_000, (0, 1, 9_000_000));
        assert_eq!(
            before,
            render_frozen(),
            "queued output must retain the program-frame selection"
        );
        cpu.stop_streams();
        assert!(service.join(Duration::from_secs(2)));
        drop(selected);
        drop(resource);
        cpu.retire();
        assert!(
            service.budget().usage().0 > 0,
            "frozen queued frame keeps its lease"
        );
        drop(frozen);
        assert_eq!(service.budget().usage().0, 0);
        assert!(service.shutdown(Duration::from_secs(1)));
    }

    #[test]
    fn streamed_finite_sources_hide_or_hold_after_the_actual_eof() {
        let service = crate::preparation::PreparationService::default();
        for completion in [Completion::Hide, Completion::HoldLast] {
            let mut cpu = CpuScene::new(1, 64 * 1024 * 1024).with_preparation(&service);
            let resource = cpu
                .prepare_animation(
                    ResourceKey { id: 1, revision: 1 },
                    &path("animated-logo-video.mkv"),
                    AnimationOptions {
                        repeat: Repeat::Count(NonZeroU32::MIN),
                        completion,
                        ..options()
                    },
                    || false,
                )
                .unwrap();
            let source = cpu.surfaces.get(&resource).unwrap();
            let stream = source.stream.as_ref().unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let frame = stream.at(60_000_000, (0, 1, 0));
                if stream.duration_known()
                    && (completion == Completion::Hide
                        || (frame.is_some() && stream.ready_at(60_000_000, (0, 1, 0))))
                {
                    assert_eq!(frame.is_none(), completion == Completion::Hide);
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            cpu.stop_streams();
            assert!(service.join(Duration::from_secs(2)));
            drop(resource);
            cpu.retire();
            assert_eq!(service.budget().usage().0, 0);
        }
    }

    #[test]
    fn long_gif_frame_delays_stream_and_seek_without_losing_the_original_timeline() {
        let mut encoded = std::fs::read(path("animated-logo.gif")).unwrap();
        let control = encoded
            .windows(3)
            .position(|bytes| bytes == [0x21, 0xf9, 0x04])
            .unwrap();
        encoded[control + 4..control + 6].copy_from_slice(&600_u16.to_le_bytes());
        let temporary = std::env::temp_dir().join(format!(
            "ffplayout-gif-{}-{}.gif",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&temporary, encoded).unwrap();
        let service = crate::preparation::PreparationService::default();
        let mut cpu = CpuScene::new(1, 64 * 1024 * 1024).with_preparation(&service);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &temporary,
                options(),
                || false,
            )
            .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap();
        let stream = source.stream.as_ref().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let frame = stream.at(6_100_000, (0, 1, 0));
            if frame.is_some() && stream.ready_at(6_100_000, (0, 1, 0)) {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        cpu.stop_streams();
        assert!(service.join(Duration::from_secs(2)));
        drop(resource);
        cpu.retire();
        std::fs::remove_file(temporary).unwrap();
    }

    #[test]
    fn memory_pressure_switches_to_streaming_and_shortens_lookahead_instead_of_failing() {
        let budget = ResourceBudget::new(64 * 1024, 16, 2);
        let service = crate::preparation::PreparationService::new(budget.clone());
        let mut cpu = CpuScene::new(1, 64 * 1024 * 1024).with_preparation(&service);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo-video.mkv"),
                options(),
                || false,
            )
            .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap();
        let stream = source.stream.as_ref().unwrap();
        for elapsed in [100_000, 1_200_000, 2_100_000] {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                stream.at(elapsed, (0, 1, 0));
                if stream.ready_at(elapsed, (0, 1, 0)) {
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(budget.usage().0 <= 64 * 1024);
            assert!(budget.usage().1 <= 16);
        }
        cpu.stop_streams();
        assert!(service.join(Duration::from_secs(2)));
        drop(resource);
        cpu.retire();
        assert_eq!(budget.usage(), (0, 0, 0));
    }

    #[test]
    fn a_large_video_streams_instead_of_retaining_its_decoded_duration() {
        let service = crate::preparation::PreparationService::default();
        let mut cpu = CpuScene::new(4, 64 * 1024 * 1024).with_preparation(&service);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo-video.mkv"),
                AnimationOptions {
                    size: Some((1280, 720)),
                    ..options()
                },
                || false,
            )
            .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap();
        let stream = source.stream.as_ref().unwrap();
        let instance = (0, 1, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let selected = stream.at(2_100_000, instance);
            if selected.is_some() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(stream.stats().1 <= 64 * 1024 * 1024);
        assert!(service.budget().usage().0 < 128 * 1024 * 1024);
        cpu.stop_streams();
        assert!(service.join(Duration::from_secs(2)));
        drop(resource);
        cpu.retire();
        assert_eq!(service.budget().usage().0, 0);
    }

    #[test]
    fn future_retake_preparation_never_replaces_a_visible_presentations_request() {
        use crate::{
            playlist::{Action, ClipSchedule, Event, Limits},
            preparation::PreparationService,
        };
        let service = PreparationService::default();
        let mut cpu = CpuScene::new(1, 1024 * 1024).with_preparation(&service);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo-video.mkv"),
                options(),
                || false,
            )
            .unwrap();
        let spec = scene(resource.clone(), 0).layers[0].spec;
        let schedule = ClipSchedule::compile(
            2_000_000,
            &[
                Event {
                    at_us: 0,
                    action: Action::Take {
                        layer: spec,
                        transition: Transition {
                            fade_out_us: 400_000,
                            ..Default::default()
                        },
                    },
                },
                Event {
                    at_us: 500_000,
                    action: Action::Stop(spec.id),
                },
                Event {
                    at_us: 880_000,
                    action: Action::Clear(spec.id),
                },
                Event {
                    at_us: 1_000_000,
                    action: Action::Take {
                        layer: spec,
                        transition: Transition::default(),
                    },
                },
            ],
            std::slice::from_ref(&resource),
            Limits::default(),
        )
        .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap();
        let stream = source.stream.as_ref().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !stream.ready_at(600_000, (0, 1, 0)) {
            let _ = cpu
                .freeze_schedule(&schedule, 600_000, Scope::default())
                .unwrap();
            assert!(
                Instant::now() < deadline,
                "future retake displaced the current request"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!stream.ready_at(0, (0, 1, 1_000_000)));
        let queued = cpu
            .freeze_schedule(&schedule, 600_000, Scope::default())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !stream.ready_at(0, (0, 1, 1_000_000)) {
            assert!(
                cpu.freeze_schedule(&schedule, 950_000, Scope::default())
                    .unwrap()
                    .is_empty()
            );
            assert!(
                Instant::now() < deadline,
                "cleared layer did not prepare its retake"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            !cpu.freeze_schedule(&schedule, 1_000_000, Scope::default())
                .unwrap()
                .is_empty()
        );
        assert!(
            !queued.is_empty(),
            "future preparation must not change queued output"
        );
        cpu.stop_streams();
        assert!(service.join(Duration::from_secs(2)));
        drop(queued);
        drop(schedule);
        drop(resource);
        cpu.retire();
        assert_eq!(service.budget().usage(), (0, 0, 0));
        assert!(service.shutdown(Duration::from_secs(2)));
    }

    #[test]
    fn scheduled_take_and_repeat_prewarm_ready_pixels_but_long_gaps_park_the_decoder() {
        use crate::{
            ffmpeg::surface::SurfaceFrame,
            playlist::{Action, ClipSchedule, Event, Limits},
            preparation::PreparationService,
        };
        let service = PreparationService::default();
        let mut cpu = CpuScene::new(2, 1024 * 1024).with_preparation(&service);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo-video.mkv"),
                options(),
                || false,
            )
            .unwrap();
        let spec = scene(resource.clone(), 0).layers[0].spec;
        let transition = Transition {
            fade_in_us: 40_000,
            hold_us: Some(200_000),
            fade_out_us: 60_000,
            pause_us: 2_090_000,
            repeat: Repeat::Count(NonZeroU32::new(2).unwrap()),
        };
        let schedule = ClipSchedule::compile(
            10_000_000,
            &[Event {
                at_us: 2_000_000,
                action: Action::Take {
                    layer: spec,
                    transition,
                },
            }],
            std::slice::from_ref(&resource),
            Limits::default(),
        )
        .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap();
        let stream = source.stream.as_ref().unwrap();
        let instance = (0, 1, 2_000_000);
        let warm = |now, elapsed| {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let frozen = cpu
                    .freeze_schedule(&schedule, now, Scope::default())
                    .unwrap();
                assert!(
                    frozen.is_empty(),
                    "preparation must not take the layer early"
                );
                if stream.ready_at(elapsed, instance) {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "scheduled pixels were not prepared"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        assert!(
            cpu.freeze_schedule(&schedule, 1_000_000, Scope::default())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            stream.stats().0,
            0,
            "must not decode outside the preparation horizon"
        );
        warm(1_600_000, 1);
        assert!(
            !cpu.freeze_schedule(&schedule, 2_020_000, Scope::default())
                .unwrap()
                .is_empty()
        );
        assert!(
            cpu.freeze_schedule(&schedule, 2_800_000, Scope::default())
                .unwrap()
                .is_empty()
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while stream.stats().1 != 0 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let idle = stream.stats().0;
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(stream.stats().0, idle);
        warm(4_000_000, 2_390_001);
        let decoded = stream.stats().0;
        // Holding a fixed future request must not continuously advance decoding.
        for now in [4_020_000, 4_100_000, 4_200_000] {
            assert!(
                cpu.freeze_schedule(&schedule, now, Scope::default())
                    .unwrap()
                    .is_empty()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(stream.stats().0, decoded);
        let primed = source.select(2_400_000, instance).unwrap();
        let input = format::input(&path("animated-logo-video.mkv")).unwrap();
        let mut reference = LogoDecoder::new(
            input,
            LogoStyle {
                scale: Some("16:16".into()),
                opacity: 1.0,
                position: "0:0".into(),
            },
            2,
            2,
        )
        .unwrap();
        let reference = loop {
            let frame = reference.next_frame().unwrap().unwrap();
            if frame.timing.contains(2_400_000) {
                break frame;
            }
        };
        for plane in 0..4 {
            for row in 0..primed.frame.plane_height(plane) as usize {
                let width = primed.frame.plane_width(plane) as usize;
                assert_eq!(
                    &primed.frame.data(plane)[row * primed.frame.stride(plane)
                        ..row * primed.frame.stride(plane) + width],
                    &reference.surface.frame.data(plane)[row * reference.surface.frame.stride(plane)
                        ..row * reference.surface.frame.stride(plane) + width],
                    "prewarming changed the animation phase"
                );
            }
        }
        let frozen = cpu
            .freeze_schedule(&schedule, 4_400_000, Scope::default())
            .unwrap();
        let mut actual = frame::Video::new(Pixel::YUV420P, 32, 32);
        let mut expected = frame::Video::new(Pixel::YUV420P, 32, 32);
        for plane in 0..3 {
            actual.data_mut(plane).fill(128);
            expected.data_mut(plane).fill(128);
        }
        assert_eq!(
            frozen
                .blend(&mut actual, ColorEncoding::Bt601Limited)
                .blended,
            1
        );
        SurfaceFrame {
            surface: primed.clone(),
            x: 0,
            y: 0,
            opacity: 0.25,
        }
        .blend(&mut expected);
        for plane in 0..3 {
            assert_eq!(actual.data(plane), expected.data(plane));
        }
        assert!(
            cpu.freeze_schedule(&schedule, 5_000_000, Scope::default())
                .unwrap()
                .is_empty()
        );
        assert!(
            cpu.freeze_schedule(&schedule, 10_000_000, Scope::default())
                .is_none()
        );
        cpu.stop_streams();
        assert!(service.join(Duration::from_secs(2)));
        drop(frozen);
        drop(primed);
        drop(schedule);
        drop(resource);
        cpu.retire();
        assert_eq!(service.budget().usage(), (0, 0, 0));
        assert!(service.shutdown(Duration::from_secs(2)));
    }

    #[test]
    fn overlapping_animation_fades_match_independent_frame_selection_at_multiple_program_rates() {
        use crate::ffmpeg::surface::SurfaceFrame;
        let budget = ResourceBudget::new(1024 * 1024, 32, 2);
        let mut cpu = CpuScene::new(4, 1024 * 1024).with_budget(budget.clone());
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo.gif"),
                options(),
                || false,
            )
            .unwrap();
        let source = cpu.surfaces.get(&resource).unwrap();
        let transition = Transition {
            fade_in_us: 80_000,
            hold_us: Some(240_000),
            fade_out_us: 120_000,
            pause_us: 160_000,
            repeat: Repeat::Forever,
        };
        let layers = (0..3)
            .map(|id| {
                let mut layer = scene(resource.clone(), id * 90_000).layers[0].clone();
                layer.spec.id = LayerId(id as u64);
                layer.spec.position = (id as i32 * 4, id as i32 * 2);
                layer.spec.z = id as i32;
                layer.spec.opacity = 0.7;
                layer.presentation = Presentation::new(id * 90_000, transition).unwrap();
                layer
            })
            .collect::<Vec<_>>();
        let scene = SceneRevision {
            number: 1,
            layers: layers.into(),
        };
        let retained = budget.usage();

        for fps in [24, 25, 30, 50, 60] {
            for index in 0..fps * 20 {
                let time = i64::from(index) * 1_000_000 / i64::from(fps);
                let mut actual = frame::Video::new(Pixel::YUV420P, 32, 32);
                let mut expected = frame::Video::new(Pixel::YUV420P, 32, 32);
                for plane in 0..3 {
                    actual.data_mut(plane).fill(128);
                    expected.data_mut(plane).fill(128);
                }
                for layer in scene.layers.iter() {
                    if let Some(opacity) = layer.presentation.opacity(time).filter(|v| *v > 0.0) {
                        let elapsed = layer.presentation.elapsed_us(time) % 600_000;
                        // Fixture delays are 100/300/200 ms; selection is independent
                        // of the source's production timeline lookup.
                        let selected = if elapsed < 100_000 {
                            0
                        } else if elapsed < 400_000 {
                            1
                        } else {
                            2
                        };
                        SurfaceFrame {
                            surface: source.frames[selected].1.clone(),
                            x: layer.spec.position.0,
                            y: layer.spec.position.1,
                            opacity: opacity * layer.spec.opacity,
                        }
                        .blend(&mut expected);
                    }
                }
                let report = cpu.blend(
                    &scene,
                    time,
                    Scope::default(),
                    &mut actual,
                    ColorEncoding::Bt601Limited,
                );
                assert_eq!((report.missing, report.unsupported), (0, 0));
                for plane in 0..3 {
                    assert_eq!(
                        actual.data(plane),
                        expected.data(plane),
                        "fps={fps}, frame={index}, plane={plane}"
                    );
                }
                assert_eq!(
                    budget.usage(),
                    retained,
                    "cached loops must not accumulate resources"
                );
            }
        }
        drop(scene);
        drop(resource);
        cpu.retire();
        assert_eq!(budget.usage(), (0, 0, 0));
    }

    #[test]
    fn repeated_stream_take_stop_clear_and_live_suspension_release_all_resources() {
        use crate::{
            preparation::PreparationService,
            scene::{Command, SceneController},
        };
        let service = PreparationService::default();
        let budget = service.budget();

        for cycle in 0..20 {
            let mut cpu = CpuScene::new(2, 1024 * 1024).with_preparation(&service);
            let resource = cpu
                .prepare_animation(
                    ResourceKey {
                        id: 1,
                        revision: cycle + 1,
                    },
                    &path("animated-logo-video.mkv"),
                    options(),
                    || false,
                )
                .unwrap();
            let source = cpu.surfaces.get(&resource).unwrap();
            let stream = source.stream.as_ref().unwrap();
            let mut controller = SceneController::new_local(2, 2);
            let spec = scene(resource.clone(), 0).layers[0].spec;
            let outcomes = controller.apply_at_frame(
                0,
                [
                    Command::Prepare(resource.key),
                    Command::Ready(resource.clone()),
                    Command::Take {
                        layer: spec,
                        transition: Transition {
                            fade_out_us: 40_000,
                            ..Default::default()
                        },
                    },
                ],
            );
            assert!(outcomes.iter().all(|ack| ack.outcome.is_ok()));
            let deadline = Instant::now() + Duration::from_secs(5);
            let queued = loop {
                let frozen =
                    cpu.freeze(&controller.advance(2_100_000), 2_100_000, Scope::default());
                if stream.ready_at(2_100_000, (0, 1, 0)) {
                    break frozen;
                }
                assert!(
                    Instant::now() < deadline,
                    "cycle {cycle} never became ready"
                );
                std::thread::sleep(Duration::from_millis(1));
            };
            // Live takeover suppresses demand while queued outputs still own pixels.
            cpu.suspend_streams();
            let deadline = Instant::now() + Duration::from_secs(2);
            while stream.stats().1 != 0 {
                assert!(
                    Instant::now() < deadline,
                    "live suppression did not empty cache"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            let decoded = stream.stats().0;
            std::thread::sleep(Duration::from_millis(10));
            assert_eq!(stream.stats().0, decoded, "inactive decoder kept running");
            assert!(
                controller.apply_at_frame(2_200_000, [Command::Stop(spec.id)])[0]
                    .outcome
                    .is_ok()
            );
            assert!(
                cpu.freeze(&controller.advance(2_240_000), 2_240_000, Scope::default())
                    .is_empty()
            );
            assert!(
                controller.apply_at_frame(
                    2_250_000,
                    [Command::Take {
                        layer: spec,
                        transition: Transition::default(),
                    }]
                )[0]
                .outcome
                .is_ok()
            );
            assert!(
                controller.apply_at_frame(2_260_000, [Command::Clear(spec.id)])[0]
                    .outcome
                    .is_ok()
            );
            // Retake must request its own source origin rather than an old ready frame.
            assert!(
                controller.apply_at_frame(
                    3_000_000,
                    [Command::Take {
                        layer: spec,
                        transition: Transition::default()
                    }]
                )[0]
                .outcome
                .is_ok()
            );
            let retaken = cpu.freeze(&controller.advance(3_000_000), 3_000_000, Scope::default());
            assert!(!retaken.is_empty());
            assert!(
                controller.apply_at_frame(3_100_000, [Command::ClearAll])[0]
                    .outcome
                    .is_ok()
            );
            assert!(
                cpu.freeze(&controller.advance(3_100_000), 3_100_000, Scope::default())
                    .is_empty()
            );
            cpu.stop_streams();
            assert!(
                service.join(Duration::from_secs(2)),
                "worker did not join in cycle {cycle}"
            );
            assert_eq!(budget.usage().2, 0);
            drop(retaken);
            drop(controller);
            drop(resource);
            cpu.retire();
            assert!(budget.usage().0 > 0, "queued output lost pixel ownership");
            drop(queued);
            assert_eq!(
                budget.usage(),
                (0, 0, 0),
                "cycle {cycle} leaked reservations"
            );
        }
        assert!(service.shutdown(Duration::from_secs(2)));
    }

    #[test]
    fn local_video_reuses_decoder_and_failure_or_cancel_releases_partial_preparation() {
        let budget = ResourceBudget::new(1024 * 1024, 1024, 1);
        let service = crate::preparation::PreparationService::new(budget.clone());
        let mut cpu = CpuScene::new(4, 1024 * 1024).with_preparation(&service);
        let resource = cpu
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &path("animated-logo-video.mkv"),
                options(),
                || false,
            )
            .unwrap();
        assert!(cpu.surfaces.get(&resource).unwrap().stream.is_some());
        drop(resource);
        cpu.retire();
        assert!(service.join(Duration::from_secs(2)));
        assert_eq!(budget.usage().0, 0);
        let calls = std::cell::Cell::new(0);
        assert!(
            cpu.prepare_animation(
                ResourceKey { id: 2, revision: 2 },
                &path("animated-logo.gif"),
                options(),
                || {
                    calls.set(calls.get() + 1);
                    calls.get() > 10
                }
            )
            .is_err()
        );
        assert_eq!(budget.usage().0, 0);
        let partial_budget = ResourceBudget::new(1024 * 1024, 1, 1);
        let mut partial = CpuScene::new(1, 1024 * 1024).with_budget(partial_budget.clone());
        assert!(
            partial
                .prepare_animation(
                    ResourceKey { id: 1, revision: 1 },
                    &path("animated-logo.gif"),
                    options(),
                    || false
                )
                .is_err()
        );
        assert_eq!(partial_budget.usage(), (0, 0, 0));
        let mut tiny = CpuScene::new(1, 1).with_budget(budget.clone());
        assert!(
            tiny.prepare_animation(
                ResourceKey { id: 3, revision: 3 },
                &path("animated-logo.gif"),
                options(),
                || false
            )
            .is_err()
        );
        assert_eq!(budget.usage().0, 0);
        assert!(
            cpu.prepare_animation(
                ResourceKey { id: 4, revision: 4 },
                &path("animated-logo.gif"),
                AnimationOptions {
                    size: Some((3, 4)),
                    ..options()
                },
                || false
            )
            .is_err()
        );
    }
}
