//! Share composited program frames across desktop, recording and encoders.
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, PoisonError},
};

use anyhow::Result;
use ffmpeg_next::{format::Pixel, frame, software::scaling};

use crate::{
    graphics::{
        clip::ClipSceneFrame,
        logo_adapter::{LogoOverlay, blend_logo},
        text_adapter::TextOverlays,
    },
    utils::ffmpeg::{make_video_frame_writable, reference_video_frame},
};

type Resolution = (u32, u32);

#[derive(Default)]
pub(super) struct VideoCompositor {
    resolutions: BTreeMap<Resolution, Arc<Mutex<ResolutionCompositor>>>,
    position: Option<String>,
}

struct ResolutionCompositor {
    text_cache: [ff_graphics::ffmpeg::text::TextScaleCache; 2],
    scaler: Option<scaling::Context>,
    frame: frame::Video,
    source_logo: Option<Arc<LogoOverlay>>,
    logo: Option<LogoOverlay>,
    position: Option<String>,
}

// SAFETY: each scaler is accessed under its resolution's mutex; final destruction
// has exclusive ownership. Different resolutions remain independently runnable.
unsafe impl Send for ResolutionCompositor {}

impl VideoCompositor {
    pub(super) fn new(position: Option<String>) -> Self {
        Self {
            position,
            ..Self::default()
        }
    }

    pub(super) fn clear(&mut self) {
        self.resolutions.clear();
    }

    fn resolution(&mut self, resolution: Resolution) -> Arc<Mutex<ResolutionCompositor>> {
        self.resolutions
            .entry(resolution)
            .or_insert_with(|| {
                Arc::new(Mutex::new(ResolutionCompositor {
                    text_cache: Default::default(),
                    scaler: None,
                    frame: frame::Video::empty(),
                    source_logo: None,
                    logo: None,
                    position: self.position.clone(),
                }))
            })
            .clone()
    }
}

impl ResolutionCompositor {
    fn prepare(
        &mut self,
        source: &frame::Video,
        logo: Option<&Arc<LogoOverlay>>,
        texts: &TextOverlays,
        scene: Option<&ClipSceneFrame>,
        opacity: f64,
        (width, height): Resolution,
    ) -> Result<frame::Video> {
        if let Some(logo) = logo {
            if !self
                .source_logo
                .as_ref()
                .is_some_and(|previous| Arc::ptr_eq(previous, logo))
            {
                self.logo = Some(logo.positioned(self.position.as_deref(), width, height)?);
                self.source_logo = Some(logo.clone());
            }
        } else {
            self.logo = None;
            self.source_logo = None;
        }
        if source.width() == width && source.height() == height {
            let mut prepared = reference_video_frame(source)?;
            make_video_frame_writable(&mut prepared)?;
            if let Some(logo) = &self.logo {
                blend_logo(&mut prepared, logo, opacity);
            }
            if let Some(scene) = scene {
                scene.blend(&mut prepared);
            }
            for (index, (text, cache)) in texts.iter().zip(&mut self.text_cache).enumerate() {
                if let Some(text) = text {
                    crate::graphics::text_adapter::measure_text(index, text, || {
                        text.blend_scaled(&mut prepared, (source.width(), source.height()), cache)
                    })?;
                }
            }

            return Ok(prepared);
        }

        if !self.scaler.as_ref().is_some_and(|scaler| {
            let input = scaler.input();
            input.width == source.width()
                && input.height == source.height()
                && input.format == source.format()
        }) {
            self.scaler = Some(scaling::Context::get(
                source.format(),
                source.width(),
                source.height(),
                Pixel::YUV420P,
                width,
                height,
                scaling::flag::Flags::BILINEAR,
            )?);
        }

        if self.frame.width() != 0 {
            make_video_frame_writable(&mut self.frame)?;
        }
        if let Some(scaler) = &mut self.scaler {
            scaler.run(source, &mut self.frame)?;
        }
        self.frame.set_pts(source.pts());
        if let Some(logo) = &self.logo {
            blend_logo(&mut self.frame, logo, opacity);
        }
        if let Some(scene) = scene {
            scene.blend(&mut self.frame);
        }
        for (index, (text, cache)) in texts.iter().zip(&mut self.text_cache).enumerate() {
            if let Some(text) = text {
                crate::graphics::text_adapter::measure_text(index, text, || {
                    text.blend_scaled(&mut self.frame, (source.width(), source.height()), cache)
                })?;
            }
        }

        reference_video_frame(&self.frame)
    }
}

/// Cache lifetime is exactly one program frame, bounded by the existing queues.
pub(super) struct ProgramVideo {
    pub(super) source: frame::Video,
    compositor: Arc<Mutex<VideoCompositor>>,
    logo: Option<Arc<LogoOverlay>>,
    opacity: f64,
    texts: TextOverlays,
    scene: Option<ClipSceneFrame>,
    prepared: BTreeMap<Resolution, Mutex<PreparedFrame>>,
}

struct PreparedFrame {
    remaining: usize,
    frame: Option<frame::Video>,
}

impl ProgramVideo {
    pub(super) fn has_visible_graphics(&self) -> bool {
        (self.opacity > 0.0 && self.logo.as_ref().is_some_and(|logo| logo.opacity() > 0))
            || self.texts.iter().any(Option::is_some)
            || self.scene.is_some()
    }

    #[cfg(test)]
    pub(super) fn new(
        source: &frame::Video,
        compositor: Arc<Mutex<VideoCompositor>>,
        logo: Option<Arc<LogoOverlay>>,
        opacity: f64,
        consumers: BTreeMap<Resolution, usize>,
    ) -> Result<Arc<Self>> {
        Self::with_graphics(
            source,
            compositor,
            logo,
            opacity,
            consumers,
            Default::default(),
            None,
        )
    }

    pub(super) fn with_graphics(
        source: &frame::Video,
        compositor: Arc<Mutex<VideoCompositor>>,
        logo: Option<Arc<LogoOverlay>>,
        opacity: f64,
        consumers: BTreeMap<Resolution, usize>,
        texts: TextOverlays,
        scene: Option<ClipSceneFrame>,
    ) -> Result<Arc<Self>> {
        Ok(Arc::new(Self {
            source: reference_video_frame(source)?,
            compositor,
            logo,
            opacity,
            texts,
            scene,
            prepared: consumers
                .into_iter()
                .filter(|(_, remaining)| *remaining > 1)
                .map(|(resolution, remaining)| {
                    (
                        resolution,
                        Mutex::new(PreparedFrame {
                            remaining,
                            frame: None,
                        }),
                    )
                })
                .collect(),
        }))
    }

    pub(super) fn frame(&self, width: u32, height: u32) -> Result<frame::Video> {
        let logo = self
            .logo
            .as_ref()
            .filter(|logo| self.opacity > 0.0 && logo.opacity() > 0);
        if logo.is_none() && self.texts.iter().all(Option::is_none) && self.scene.is_none() {
            return reference_video_frame(&self.source);
        }
        let key = (width, height);
        let Some(slot) = self.prepared.get(&key) else {
            return self.prepare(logo, key);
        };
        let mut slot = slot.lock().unwrap_or_else(PoisonError::into_inner);
        slot.remaining = slot.remaining.saturating_sub(1);
        if let Some(frame) = &slot.frame {
            let result = reference_video_frame(frame)?;
            if slot.remaining == 0 {
                slot.frame = None;
            }

            return Ok(result);
        }

        let frame = self.prepare(logo, key)?;
        // The last consumer owns a reference. Other resolutions must not retain
        // this cached image merely because their output is slower.
        if slot.remaining == 0 {
            return Ok(frame);
        }
        let result = reference_video_frame(&frame)?;
        slot.frame = Some(frame);

        Ok(result)
    }

    fn prepare(
        &self,
        logo: Option<&Arc<LogoOverlay>>,
        resolution: Resolution,
    ) -> Result<frame::Video> {
        // Hold the map lock only for lookup; scaling at different resolutions
        // must not serialize otherwise independent encoding groups.
        let compositor = self
            .compositor
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .resolution(resolution);
        compositor
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .prepare(
                &self.source,
                logo,
                &self.texts,
                self.scene.as_ref(),
                self.opacity,
                resolution,
            )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::utils::config::LogoConfig;

    #[test]
    fn scrolling_text_wider_than_8192_pixels_keeps_scaled_outputs_running() {
        use ff_graphics::{
            animation::TextPreparation,
            ffmpeg::text::TextOverlay,
            style::{TextScroll, TextStyle},
        };

        let overlay = TextOverlay::prepare(
            &TextStyle {
                scroll: TextScroll::RightToLeft {
                    pixels_per_second: 100,
                },
                ..Default::default()
            },
            &"W".repeat(600),
            TextPreparation {
                canvas_width: 1920,
                canvas_height: 1080,
                frames_per_second: 25,
                timing: Default::default(),
            },
        )
        .unwrap()
        .unwrap();
        assert!(u64::from(overlay.dimensions().0) * 1280 / 1920 > 8192);
        let mut source = frame::Video::new(Pixel::YUV420P, 1920, 1080);
        for plane in 0..3 {
            source.data_mut(plane).fill(128);
        }
        let program = ProgramVideo::with_graphics(
            &source,
            Arc::new(Mutex::new(VideoCompositor::default())),
            None,
            0.0,
            BTreeMap::from([((1280, 720), 2)]),
            [overlay.snapshot(500, 500), None],
            None,
        )
        .unwrap();
        let first = program.frame(1280, 720).unwrap();
        let shared = program.frame(1280, 720).unwrap();
        assert_eq!(first.data(0).as_ptr(), shared.data(0).as_ptr());
        for video in [
            first,
            program.frame(640, 360).unwrap(),
            program.frame(1920, 1080).unwrap(),
        ] {
            assert!((0..video.height() as usize).any(|row| {
                video.data(0)[row * video.stride(0)..row * video.stride(0) + video.width() as usize]
                    .iter()
                    .any(|pixel| *pixel != 128)
            }));
        }
        assert!(source.data(0).iter().all(|pixel| *pixel == 128));
    }

    #[test]
    fn clip_scenes_share_composed_pixels_and_scale_once_per_resolution() {
        let graphics = crate::graphics::clip::tests::fixture((320, 180));
        let scene = graphics.frame_at(120_000).unwrap();
        let (source, _, compositor) = setup();
        let program = ProgramVideo::with_graphics(
            &source,
            compositor.clone(),
            None,
            0.0,
            BTreeMap::from([((320, 180), 2)]),
            Default::default(),
            Some(scene.clone()),
        )
        .unwrap();
        let first = program.frame(320, 180).unwrap();
        let second = program.frame(320, 180).unwrap();
        assert_eq!(first.data(0).as_ptr(), second.data(0).as_ptr());
        assert_ne!(first.data(0), source.data(0));
        for (width, height) in [(320, 180), (160, 90)] {
            let actual = program.frame(width, height).unwrap();
            let mut expected = frame::Video::new(Pixel::YUV420P, width, height);
            for plane in 0..3 {
                expected.data_mut(plane).fill(128);
            }
            scene.blend(&mut expected);
            for plane in 0..3 {
                for row in 0..actual.plane_height(plane) as usize {
                    let columns = actual.plane_width(plane) as usize;
                    assert_eq!(
                        &actual.data(plane)
                            [row * actual.stride(plane)..row * actual.stride(plane) + columns],
                        &expected.data(plane)
                            [row * expected.stride(plane)..row * expected.stride(plane) + columns],
                    );
                }
            }
        }
        let without_scene = ProgramVideo::with_graphics(
            &source,
            compositor,
            None,
            0.0,
            BTreeMap::new(),
            Default::default(),
            None,
        )
        .unwrap();
        assert!(!without_scene.has_visible_graphics());
        assert_eq!(
            without_scene.frame(320, 180).unwrap().data(0),
            source.data(0)
        );
    }

    #[test]
    #[ignore = "manual release-mode comparison; not a timing assertion"]
    fn benchmark_shared_logo_composition() {
        use std::{hint::black_box, time::Instant};

        let (_, logo, compositor) = setup();
        let mut source = frame::Video::new(Pixel::YUV420P, 1280, 720);
        for plane in 0..source.planes() {
            source.data_mut(plane).fill(128);
        }
        let logo = Arc::new(logo.positioned(Some("W-w-8:H-h-8"), 1280, 720).unwrap());
        for groups in [1, 2, 4] {
            let mut old_times = Vec::new();
            let mut new_times = Vec::new();
            for round in 0..6 {
                // Alternate order; the first repetition warms buffers and caches.
                for shared in if round % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let started = Instant::now();
                    for _ in 0..300 {
                        let mut held = Vec::with_capacity(groups);
                        if shared {
                            let program = ProgramVideo::new(
                                &source,
                                compositor.clone(),
                                Some(logo.clone()),
                                1.0,
                                BTreeMap::from([((1280, 720), groups)]),
                            )
                            .unwrap();
                            for _ in 0..groups {
                                held.push(program.frame(1280, 720).unwrap());
                            }
                        } else {
                            for _ in 0..groups {
                                let mut frame = reference_video_frame(&source).unwrap();
                                make_video_frame_writable(&mut frame).unwrap();
                                blend_logo(&mut frame, &logo, 1.0);
                                held.push(frame);
                            }
                        }
                        black_box(&held);
                    }
                    if round > 0 {
                        if shared {
                            new_times.push(started.elapsed());
                        } else {
                            old_times.push(started.elapsed());
                        }
                    }
                }
            }
            old_times.sort_unstable();
            new_times.sort_unstable();
            println!(
                "groups={groups} frames=300 old_ms={:.3} shared_ms={:.3} speedup={:.2}",
                old_times[2].as_secs_f64() * 1000.0,
                new_times[2].as_secs_f64() * 1000.0,
                old_times[2].as_secs_f64() / new_times[2].as_secs_f64()
            );
        }
    }

    fn setup() -> (frame::Video, Arc<LogoOverlay>, Arc<Mutex<VideoCompositor>>) {
        ffmpeg_next::init().unwrap();
        let mut source = frame::Video::new(Pixel::YUV420P, 320, 180);
        for plane in 0..source.planes() {
            source.data_mut(plane).fill(128);
        }
        source.set_pts(Some(42));
        let config = LogoConfig {
            path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/logo.png")
                .to_string_lossy()
                .into_owned(),
            scale: Some("32:16".into()),
            opacity: 1.0,
            position: "W-w-8:H-h-8".into(),
        };
        let logo = Arc::new(LogoOverlay::load(&config, 320, 180).unwrap());
        let compositor = Arc::new(Mutex::new(VideoCompositor::new(Some(config.position))));

        (source, logo, compositor)
    }

    #[test]
    fn text_is_above_logo_at_each_resolution_and_snapshots_remain_immutable() {
        use ff_graphics::{
            animation::TextPreparation,
            ffmpeg::text::{TextOverlay, TextScaleCache},
            style::{TextPosition, TextStyle},
        };
        let (source, logo, compositor) = setup();
        let overlay = TextOverlay::prepare(
            &TextStyle {
                font_size: 36.0,
                position_x: TextPosition::End(0),
                position_y: TextPosition::End(0),
                ..TextStyle::default()
            },
            "MMMMMMMM",
            TextPreparation {
                canvas_width: 320,
                canvas_height: 180,
                frames_per_second: 25,
                timing: Default::default(),
            },
        )
        .unwrap()
        .unwrap();
        let texts = [overlay.snapshot(0, 0), None];
        let program = ProgramVideo::with_graphics(
            &source,
            compositor.clone(),
            Some(logo.clone()),
            1.0,
            BTreeMap::new(),
            texts.clone(),
            None,
        )
        .unwrap();
        assert!(program.has_visible_graphics());
        for (width, height) in [(320, 180), (160, 90)] {
            let actual = program.frame(width, height).unwrap();
            let mut expected = frame::Video::new(Pixel::YUV420P, width, height);
            for plane in 0..3 {
                expected.data_mut(plane).fill(128);
            }
            let positioned = logo.positioned(Some("W-w-8:H-h-8"), width, height).unwrap();
            blend_logo(&mut expected, &positioned, 1.0);
            texts[0]
                .as_ref()
                .unwrap()
                .blend_scaled(&mut expected, (320, 180), &mut TextScaleCache::default())
                .unwrap();
            let mut reversed = frame::Video::new(Pixel::YUV420P, width, height);
            for plane in 0..3 {
                reversed.data_mut(plane).fill(128);
            }
            texts[0]
                .as_ref()
                .unwrap()
                .blend_scaled(&mut reversed, (320, 180), &mut TextScaleCache::default())
                .unwrap();
            blend_logo(&mut reversed, &positioned, 1.0);
            assert_ne!(
                expected.data(0),
                reversed.data(0),
                "test layers must overlap"
            );
            for plane in 0..3 {
                for row in 0..actual.plane_height(plane) as usize {
                    let columns = actual.plane_width(plane) as usize;
                    assert_eq!(
                        &actual.data(plane)
                            [row * actual.stride(plane)..row * actual.stride(plane) + columns],
                        &expected.data(plane)
                            [row * expected.stride(plane)..row * expected.stride(plane) + columns]
                    );
                }
            }
        }
        let hidden = ProgramVideo::with_graphics(
            &source,
            compositor.clone(),
            None,
            0.0,
            BTreeMap::new(),
            Default::default(),
            None,
        )
        .unwrap();
        assert!(!hidden.has_visible_graphics());
        assert_eq!(hidden.frame(320, 180).unwrap().data(0), source.data(0));
        let text_only = ProgramVideo::with_graphics(
            &source,
            compositor,
            None,
            0.0,
            BTreeMap::new(),
            texts,
            None,
        )
        .unwrap();
        assert!(text_only.has_visible_graphics());
        assert_ne!(text_only.frame(320, 180).unwrap().data(0), source.data(0));
    }

    #[test]
    fn independent_output_resolutions_each_receive_only_their_own_logo() {
        let (_, logo, compositor) = setup();
        let mut source = frame::Video::new(Pixel::YUV420P, 1280, 720);
        for plane in 0..source.planes() {
            source.data_mut(plane).fill(128);
        }
        for _ in 0..3 {
            let program = ProgramVideo::new(
                &source,
                compositor.clone(),
                Some(logo.clone()),
                0.5,
                BTreeMap::new(),
            )
            .unwrap();
            for (width, height) in [(1280, 720), (854, 480), (480, 270)] {
                let actual = program.frame(width, height).unwrap();
                let mut expected = frame::Video::new(Pixel::YUV420P, width, height);
                for plane in 0..expected.planes() {
                    expected.data_mut(plane).fill(128);
                }
                let positioned = logo.positioned(Some("W-w-8:H-h-8"), width, height).unwrap();
                blend_logo(&mut expected, &positioned, 0.5);
                for plane in 0..actual.planes() {
                    for row in 0..actual.plane_height(plane) as usize {
                        let columns = actual.plane_width(plane) as usize;
                        assert_eq!(
                            &actual.data(plane)
                                [row * actual.stride(plane)..row * actual.stride(plane) + columns],
                            &expected.data(plane)[row * expected.stride(plane)
                                ..row * expected.stride(plane) + columns],
                            "{width}x{height}, plane {plane}, row {row}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn same_resolution_reuses_pixels_and_other_resolutions_keep_logo_pixel_size() {
        let (source, logo, compositor) = setup();
        let program = ProgramVideo::new(
            &source,
            compositor.clone(),
            Some(logo),
            1.0,
            BTreeMap::from([((320, 180), 2), ((160, 90), 1)]),
        )
        .unwrap();
        let large = program.frame(320, 180).unwrap();
        let second = program.frame(320, 180).unwrap();
        assert_eq!(large.data(0).as_ptr(), second.data(0).as_ptr());
        let small = program.frame(160, 90).unwrap();
        assert_eq!(
            (small.width(), small.height(), small.pts()),
            (160, 90, Some(42))
        );
        let compositor = compositor.lock().unwrap();
        for ((width, height), state) in &compositor.resolutions {
            let state = state.lock().unwrap();
            let logo = state.logo.as_ref().unwrap();
            assert_eq!((logo.width(), logo.height()), (32, 16));
            assert_eq!(
                (logo.position().0, logo.position().1),
                (width - 32 - 8, height - 16 - 8)
            );
        }
        assert!(source.data(0).iter().all(|value| *value == 128));
        assert!(large.data(0).iter().any(|value| *value != 128));
        assert!(small.data(0).iter().any(|value| *value != 128));
        assert!(
            program
                .prepared
                .values()
                .all(|slot| slot.lock().unwrap().frame.is_none())
        );
    }

    #[test]
    fn subsequent_frames_and_fades_do_not_overwrite_retained_encoder_frames() {
        let (source, logo, compositor) = setup();
        let first = ProgramVideo::new(
            &source,
            compositor.clone(),
            Some(logo.clone()),
            1.0,
            BTreeMap::new(),
        )
        .unwrap();
        let retained = first.frame(160, 90).unwrap();
        let pixels = retained.data(0).to_vec();
        let faded = ProgramVideo::new(
            &source,
            compositor.clone(),
            Some(logo.clone()),
            0.5,
            BTreeMap::new(),
        )
        .unwrap();
        let faded = faded.frame(160, 90).unwrap();
        assert_eq!(retained.data(0), pixels);
        assert_ne!(retained.data(0), faded.data(0));
        let hidden = ProgramVideo::new(
            &source,
            compositor.clone(),
            Some(logo),
            0.0,
            BTreeMap::new(),
        )
        .unwrap();
        let hidden_frame = hidden.frame(160, 90).unwrap();
        assert_eq!(hidden_frame.data(0).as_ptr(), source.data(0).as_ptr());
        assert!(hidden.prepared.is_empty());
        drop(first);
        compositor.lock().unwrap().clear();
        assert_eq!(retained.data(0), pixels);
    }

    #[test]
    fn concurrent_encoding_groups_share_one_composition() {
        let (source, logo, compositor) = setup();
        let program = ProgramVideo::new(
            &source,
            compositor,
            Some(logo),
            1.0,
            BTreeMap::from([((160, 90), 4)]),
        )
        .unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let program = program.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    program.frame(160, 90).unwrap()
                })
            })
            .collect();
        let frames: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        assert!(
            frames
                .iter()
                .all(|frame| frame.data(0).as_ptr() == frames[0].data(0).as_ptr())
        );
    }

    #[test]
    fn another_resolution_does_not_wait_for_a_busy_compositor() {
        let (source, logo, compositor) = setup();
        let program = ProgramVideo::new(
            &source,
            compositor.clone(),
            Some(logo),
            1.0,
            BTreeMap::from([((320, 180), 1), ((160, 90), 1)]),
        )
        .unwrap();
        let large = compositor.lock().unwrap().resolution((320, 180));
        let held = large.lock().unwrap();
        let (done, receive) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            done.send(program.frame(160, 90).unwrap()).unwrap();
        });
        let result = receive.recv_timeout(std::time::Duration::from_secs(1));
        drop(held);
        worker.join().unwrap();
        assert!(result.is_ok(), "independent resolutions were serialized");
    }
}
