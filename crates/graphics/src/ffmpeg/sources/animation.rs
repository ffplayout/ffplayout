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
                && (frames.len() >= 256
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
