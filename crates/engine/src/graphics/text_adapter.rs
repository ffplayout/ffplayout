//! Engine adapter: resolve media-derived content before calling the graphics renderer.
use std::{
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use ff_graphics::preparation::{Preparation, PreparationService, ResourceBudget};
use ffmpeg_next::frame;
use regex::Regex;

use ff_graphics::{ffmpeg::text::TextOverlay as PreparedTextOverlay, style::TextStyle};

use crate::utils::config::TextConfig;

pub(crate) use ff_graphics::text::{available_font_families, init};
#[cfg(feature = "desktop-base")]
pub(crate) use ff_graphics::text::{render_left_aligned_text_bitmap, render_wrapped_text_bitmap};

pub(crate) use ff_graphics::animation::{TextPreparation, TextTiming};
pub(crate) use ff_graphics::ffmpeg::text::{TextFrame, TextOverlays};

pub(crate) fn measure_text<T>(index: usize, text: &TextFrame, blend: impl FnOnce() -> T) -> T {
    let (width, height) = text.dimensions();
    let stage = if index == 0 {
        crate::benchmark::Stage::TextStatic
    } else {
        crate::benchmark::Stage::TextRuntime
    };
    crate::benchmark::measure_overlay(stage, width, height, blend)
}

pub(crate) fn blend_text_overlays(target: &mut frame::Video, texts: &TextOverlays) {
    for (index, text) in texts.iter().enumerate() {
        if let Some(text) = text {
            measure_text(index, text, || text.blend(target));
        }
    }
}

pub struct TextOverlay(PreparedTextOverlay);

/// Latest request owns its publication ticket. Old pixels remain active while
/// rendering; replacing/dropping the ticket cancels obsolete publication.
#[derive(Default)]
pub(crate) struct AsyncTextOverlay {
    request: Option<TextRequest>,
    pending: Option<Preparation<Option<Arc<TextOverlay>>>>,
    ready: Option<Arc<TextOverlay>>,
}

struct TextRequest {
    service: PreparationService,
    config: TextConfig,
    label: String,
    context: TextPreparation,
    retry_at: Instant,
}

impl AsyncTextOverlay {
    pub(crate) fn request(
        &mut self,
        service: &PreparationService,
        config: Option<&TextConfig>,
        label: &str,
        context: TextPreparation,
    ) -> Result<()> {
        self.pending = None;
        self.request = None;
        let Some(config) = config else {
            self.ready = None;
            return Ok(());
        };
        self.request = Some(TextRequest {
            service: service.clone(),
            config: config.clone(),
            label: label.to_owned(),
            context,
            retry_at: Instant::now(),
        });
        self.schedule();

        Ok(())
    }

    fn schedule(&mut self) {
        let Some(request) = &mut self.request else {
            return;
        };

        if self.pending.is_some() || Instant::now() < request.retry_at {
            return;
        }

        let config = request.config.clone();
        let label = request.label.clone();
        let context = request.context;
        let budget = request.service.budget();

        match request.service.submit(move |cancelled, shutdown| {
            if cancelled.is_cancelled() || shutdown.is_cancelled() {
                return Ok(None);
            }
            TextOverlay::prepare_budgeted(&config, &label, context, &budget)
                .map(|prepared| prepared.map(Arc::new))
        }) {
            Ok(pending) => {
                self.pending = Some(pending);

                self.request = None;
            }
            Err(_) => request.retry_at = Instant::now() + Duration::from_millis(250),
        }
    }

    fn poll(&mut self) {
        let Some(result) = self.pending.as_ref().and_then(Preparation::result).cloned() else {
            return;
        };
        self.pending = None;

        match result {
            Ok(ready) => self.ready = ready,
            Err(error) => {
                log::warn!("Text graphics preparation failed; retaining previous surface: {error}");
            }
        }
    }

    pub(crate) fn snapshot(&mut self, program_frame: i64, scroll_frame: i64) -> Option<TextFrame> {
        self.schedule();
        self.poll();

        self.ready.as_ref()?.snapshot(program_frame, scroll_frame)
    }

    pub(crate) fn is_some(&self) -> bool {
        self.ready.is_some() || self.pending.is_some() || self.request.is_some()
    }
}

impl TextOverlay {
    #[cfg(any(test, feature = "desktop-base"))]
    pub fn prepare(
        config: &TextConfig,
        media_path: &str,
        context: TextPreparation,
    ) -> Result<Option<Self>> {
        Self::prepare_with_budget(config, media_path, context, None)
    }

    fn prepare_with_budget(
        config: &TextConfig,
        media_path: &str,
        context: TextPreparation,
        budget: Option<&ResourceBudget>,
    ) -> Result<Option<Self>> {
        let Some(text) = overlay_text(config, media_path) else {
            return Ok(None);
        };
        let style = TextStyle {
            font_family: config.font_family.clone(),
            font_weight: config.font_weight,
            font_size: config.font_size,
            line_spacing: config.line_spacing,
            text_color: config.text_color,
            opacity: config.opacity,
            position_x: config.position_x,
            position_y: config.position_y,
            background: config.background,
            scroll: config.scroll,
            scroll_repeat: config.scroll_repeat,
            fade_in_seconds: config.fade_in_seconds,
            fade_out_seconds: config.fade_out_seconds,
        };

        Ok(PreparedTextOverlay::prepare_budgeted(&style, &text, context, budget)?.map(Self))
    }

    fn prepare_budgeted(
        config: &TextConfig,
        label: &str,
        context: TextPreparation,
        budget: &ResourceBudget,
    ) -> Result<Option<Self>> {
        Self::prepare_with_budget(config, label, context, Some(budget))
    }

    pub fn snapshot(&self, program_frame: i64, scroll_frame: i64) -> Option<TextFrame> {
        self.0.snapshot(program_frame, scroll_frame)
    }

    #[cfg(any(test, feature = "desktop-base"))]
    pub fn blend(&mut self, target: &mut frame::Video, program_frame: i64, scroll_frame: i64) {
        self.0.blend(target, program_frame, scroll_frame);
    }

    #[cfg(test)]
    pub fn dimensions(&self) -> (u32, u32) {
        self.0.dimensions()
    }
}

fn overlay_text(config: &TextConfig, media_path: &str) -> Option<String> {
    if config.use_filename {
        if let Some(regex) = &config.filename_regex
            && let Ok(regex) = Regex::new(regex)
            && let Some(captures) = regex.captures(media_path)
            && let Some(capture) = captures.get(1)
        {
            return Some(capture.as_str().to_string());
        }

        return Path::new(media_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned);
    }

    config.text.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::config::{
        RgbaColor, TextBackgroundConfig, TextPosition, TextScroll, TextWeight,
    };
    use ffmpeg_next::util::format::pixel::Pixel;

    fn context() -> TextPreparation {
        TextPreparation {
            canvas_width: 320,
            canvas_height: 180,
            frames_per_second: 25,
            timing: TextTiming::default(),
        }
    }

    fn wait_overlay(overlay: &mut AsyncTextOverlay, at: i64) -> Option<TextFrame> {
        let deadline = Instant::now() + Duration::from_secs(5);

        loop {
            let snapshot = overlay.snapshot(at, at);

            if overlay.pending.is_none() && overlay.request.is_none() {
                return snapshot;
            }

            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn block_service(
        service: &PreparationService,
    ) -> (Preparation<()>, std::sync::mpsc::Sender<()>) {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let operation = Arc::new(std::sync::Mutex::new(Some((entered_tx, release_rx))));
        let deadline = Instant::now() + Duration::from_secs(2);
        let pending = loop {
            let operation = operation.clone();

            if let Ok(pending) = service.submit(move |_, _| {
                let (entered, release) = operation.lock().unwrap().take().unwrap();
                entered.send(()).unwrap();
                release.recv().unwrap();
                Ok(())
            }) {
                break pending;
            }

            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        (pending, release_tx)
    }

    #[test]
    fn async_replacement_keeps_old_pixels_cancels_obsolete_results_and_clear_releases_leases() {
        let service = PreparationService::default();
        let mut overlay = AsyncTextOverlay::default();
        let mut config = TextConfig {
            text: Some("first".into()),
            ..Default::default()
        };
        overlay
            .request(&service, Some(&config), "clip", context())
            .unwrap();
        let queued = wait_overlay(&mut overlay, 25).unwrap();
        let (blocker, release) = block_service(&service);
        config.text = Some("obsolete".into());
        overlay
            .request(&service, Some(&config), "clip", context())
            .unwrap();
        assert!(overlay.snapshot(26, 26).unwrap().same_surface(&queued));
        config.text = Some("newest".into());
        overlay
            .request(&service, Some(&config), "clip", context())
            .unwrap();
        assert!(overlay.snapshot(27, 27).unwrap().same_surface(&queued));
        release.send(()).unwrap();
        let newest = wait_overlay(&mut overlay, 28).unwrap();
        assert!(!newest.same_surface(&queued));
        let synchronous = TextOverlay::prepare(&config, "clip", context())
            .unwrap()
            .unwrap()
            .snapshot(28, 28)
            .unwrap();
        assert_eq!(newest.dimensions(), synchronous.dimensions());
        overlay.request(&service, None, "clip", context()).unwrap();
        assert!(overlay.snapshot(29, 29).is_none());
        drop(newest);
        assert!(
            service.budget().usage().0 > 0,
            "queued old frames still own pixel leases"
        );
        drop(queued);
        drop(blocker);
        assert!(service.shutdown(Duration::from_secs(2)));
        assert_eq!(service.budget().usage(), (0, 0, 0));
    }

    #[test]
    fn clear_while_loading_cannot_publish_late_and_invalid_text_keeps_the_last_good_surface() {
        let service = PreparationService::default();
        let mut overlay = AsyncTextOverlay::default();
        let mut config = TextConfig {
            text: Some("visible".into()),
            ..Default::default()
        };
        overlay
            .request(&service, Some(&config), "clip", context())
            .unwrap();
        let first = wait_overlay(&mut overlay, 25).unwrap();
        config.font_size = f32::NAN;
        overlay
            .request(&service, Some(&config), "clip", context())
            .unwrap();
        assert!(wait_overlay(&mut overlay, 26).unwrap().same_surface(&first));
        let (blocker, release) = block_service(&service);
        config.font_size = 20.0;
        overlay
            .request(&service, Some(&config), "clip", context())
            .unwrap();
        overlay.request(&service, None, "clip", context()).unwrap();
        release.send(()).unwrap();
        assert!(service.shutdown(Duration::from_secs(2)));
        assert!(overlay.snapshot(27, 27).is_none());
        drop(first);
        drop(blocker);
        assert_eq!(service.budget().usage(), (0, 0, 0));
    }

    #[test]
    fn queue_saturation_is_retried_and_late_completion_uses_the_original_program_clock() {
        let service = PreparationService::new(ResourceBudget::new(8 * 1024 * 1024, 16, 1));
        let (blocker, release) = block_service(&service);
        let mut overlay = AsyncTextOverlay::default();
        let config = TextConfig {
            text: Some("fade and scroll".into()),
            fade_in_seconds: 1.0,
            scroll: TextScroll::RightToLeft {
                pixels_per_second: 80,
            },
            ..Default::default()
        };
        overlay
            .request(&service, Some(&config), "clip", context())
            .unwrap();
        assert!(overlay.request.is_some());
        assert!(overlay.snapshot(0, 0).is_none());
        release.send(()).unwrap();
        let actual = wait_overlay(&mut overlay, 20).unwrap();
        let expected = TextOverlay::prepare(&config, "clip", context())
            .unwrap()
            .unwrap()
            .snapshot(20, 20)
            .unwrap();
        let blend = |text: TextFrame| {
            let mut target = frame::Video::new(Pixel::YUV420P, 320, 180);

            for plane in 0..3 {
                target.data_mut(plane).fill(128);
            }

            text.blend(&mut target);

            target
        };
        let actual = blend(actual);
        let expected = blend(expected);

        for plane in 0..3 {
            for row in 0..actual.plane_height(plane) as usize {
                let width = actual.plane_width(plane) as usize;
                assert_eq!(
                    &actual.data(plane)
                        [row * actual.stride(plane)..row * actual.stride(plane) + width],
                    &expected.data(plane)
                        [row * expected.stride(plane)..row * expected.stride(plane) + width]
                );
            }
        }

        drop(blocker);
        drop(overlay);
        assert!(service.shutdown(Duration::from_secs(2)));
        assert_eq!(service.budget().usage(), (0, 0, 0));
    }

    #[test]
    fn media_text_selection_stays_in_the_engine() {
        let mut config = TextConfig {
            text: Some("configured".into()),
            ..TextConfig::default()
        };
        assert_eq!(
            overlay_text(&config, "/media/title.mp4").as_deref(),
            Some("configured")
        );
        config.use_filename = true;
        config.filename_regex = Some("/([^/]+)\\.mp4$".into());
        assert_eq!(
            overlay_text(&config, "/media/title.mp4").as_deref(),
            Some("title")
        );
        config.filename_regex = Some("[".into());
        assert_eq!(
            overlay_text(&config, "/media/title.mp4").as_deref(),
            Some("title.mp4")
        );
    }

    #[test]
    fn adapter_preserves_style_and_program_frame_timing() {
        let style = TextStyle {
            font_size: 20.0,
            font_weight: TextWeight::Bold,
            line_spacing: 3.0,
            text_color: RgbaColor {
                r: 120,
                g: 220,
                b: 50,
                a: 200,
            },
            opacity: 0.7,
            position_x: TextPosition::Center,
            position_y: TextPosition::End(6),
            background: Some(TextBackgroundConfig {
                color: RgbaColor {
                    r: 10,
                    g: 20,
                    b: 30,
                    a: 170,
                },
                padding: 4,
            }),
            scroll: TextScroll::RightToLeft {
                pixels_per_second: 80,
            },
            scroll_repeat: 2,
            fade_in_seconds: 1.0,
            fade_out_seconds: 0.5,
            ..TextStyle::default()
        };
        let config = TextConfig {
            text: Some("ignored".into()),
            use_filename: true,
            filename_regex: Some("/([^/]+)\\.mp4$".into()),
            font_family: style.font_family.clone(),
            font_weight: style.font_weight,
            font_size: style.font_size,
            line_spacing: style.line_spacing,
            text_color: style.text_color,
            opacity: style.opacity,
            position_x: style.position_x,
            position_y: style.position_y,
            background: style.background,
            scroll: style.scroll,
            scroll_repeat: style.scroll_repeat,
            fade_in_seconds: style.fade_in_seconds,
            fade_out_seconds: style.fade_out_seconds,
        };
        let mut adapter = TextOverlay::prepare(
            &config,
            "/media/Title.mp4",
            TextPreparation {
                canvas_width: 320,
                canvas_height: 180,
                frames_per_second: 25,
                timing: TextTiming {
                    fade_start_frame: 10,
                    scroll_start_frame: 10,
                    end_frame: Some(100),
                },
            },
        )
        .unwrap()
        .unwrap();
        let mut direct = PreparedTextOverlay::prepare(
            &style,
            "Title",
            TextPreparation {
                canvas_width: 320,
                canvas_height: 180,
                frames_per_second: 25,
                timing: TextTiming {
                    fade_start_frame: 10,
                    scroll_start_frame: 10,
                    end_frame: Some(100),
                },
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(adapter.dimensions(), direct.dimensions());

        for pts in [10, 25, 60, 95, 100] {
            let mut actual = frame::Video::new(Pixel::YUV420P, 320, 180);
            let mut expected = frame::Video::new(Pixel::YUV420P, 320, 180);

            for plane in 0..3 {
                actual
                    .data_mut(plane)
                    .fill(if plane == 0 { 50 } else { 128 });
                expected
                    .data_mut(plane)
                    .fill(if plane == 0 { 50 } else { 128 });
            }

            adapter.blend(&mut actual, pts, pts);
            direct.blend(&mut expected, pts, pts);

            for plane in 0..3 {
                assert_eq!(
                    actual.data(plane),
                    expected.data(plane),
                    "pts={pts}, plane={plane}"
                );
            }
        }
    }
}
