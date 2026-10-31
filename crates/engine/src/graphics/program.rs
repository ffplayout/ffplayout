//! Select one immutable logo surface on the final program timeline before fan-out.
use anyhow::Result;
use ffmpeg_next::{Rational, Rescale, frame};

use crate::{
    clock::SharedPlayoutClock,
    graphics::text_adapter::TextOverlays,
    input::diagnostics::LiveInputDiagnostics,
    output::FrameOutput,
    utils::{
        config::OutputConfig,
        ffmpeg::{make_video_frame_writable, reference_video_frame},
    },
};

use super::source::ProgramGraphicsState;

pub(crate) struct ProgramGraphicsOutput<'a, O> {
    output: &'a mut O,
    state: &'a mut ProgramGraphicsState,
    time_base: Rational,
    opacity: f64,
    texts: TextOverlays,
}

impl<'a, O: FrameOutput> ProgramGraphicsOutput<'a, O> {
    pub(crate) fn new(
        output: &'a mut O,
        state: &'a mut ProgramGraphicsState,
        config: &OutputConfig,
    ) -> Self {
        state.configure(
            config.logo.as_ref(),
            config.width,
            config.height,
            config.channel_id,
        );
        if !state.has_source() {
            output.clear_logo_overlay();
        }

        Self {
            output,
            state,
            time_base: config.video_time_base,
            opacity: 1.0,
            texts: Default::default(),
        }
    }

    fn prepare(&mut self, source: &frame::Video) -> Result<Option<frame::Video>> {
        let time = source
            .pts()
            .unwrap_or_default()
            .rescale(self.time_base, Rational(1, 1_000_000));
        self.state.set_visible(self.opacity > 0.0);
        let logo = self.state.at(time);
        let logo = logo.filter(|_| self.opacity > 0.0);
        let logo_deferred = if let Some(logo) = &logo {
            self.output.defer_shared_logo_overlay(logo, self.opacity)
        } else {
            self.output.clear_logo_overlay();
            true
        };
        if logo_deferred && self.output.defer_text_overlays(&self.texts) {
            return Ok(None);
        }
        let has_text = self.texts.iter().any(Option::is_some);
        if logo.is_none() && !has_text {
            return Ok(None);
        }
        // An immediate compositor must own both layers so that text is always
        // above the logo. Never defer a logo beneath text already in the pixels.
        self.output.clear_logo_overlay();
        let mut prepared = reference_video_frame(source)?;
        make_video_frame_writable(&mut prepared)?;
        if let Some(logo) = logo {
            crate::benchmark::measure_overlay(
                crate::benchmark::Stage::LogoOverlay,
                logo.width(),
                logo.height(),
                || {
                    super::logo_adapter::blend_logo(&mut prepared, &logo, self.opacity);
                },
            );
        }
        super::text_adapter::blend_text_overlays(&mut prepared, &self.texts);

        Ok(Some(prepared))
    }
}

impl<O: FrameOutput> FrameOutput for ProgramGraphicsOutput<'_, O> {
    fn retains_text_overlays(&self) -> bool {
        true
    }

    fn set_text_overlays(&mut self, overlays: TextOverlays) {
        self.texts = overlays;
    }

    fn apply_text_overlays(&mut self, _: &mut frame::Video, overlays: TextOverlays) {
        self.set_text_overlays(overlays);
    }

    fn set_frame_timeline(&mut self, timeline: &crate::playout::Timeline) {
        self.output.set_frame_timeline(timeline);
    }

    fn set_logo_opacity(&mut self, opacity: f64) {
        self.opacity = opacity;
    }

    fn playout_clock(&self) -> Option<SharedPlayoutClock> {
        self.output.playout_clock()
    }

    fn input_diagnostics_handle(&self) -> Option<std::sync::Arc<LiveInputDiagnostics>> {
        self.output.input_diagnostics_handle()
    }

    fn input_diagnostics(&self) -> Option<&LiveInputDiagnostics> {
        self.output.input_diagnostics()
    }

    fn audio_frame_size(&self) -> usize {
        self.output.audio_frame_size()
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        let prepared = self.prepare(frame)?;
        self.output.encode_video(prepared.as_ref().unwrap_or(frame))
    }

    fn try_encode_video(&mut self, frame: &frame::Video) -> Result<bool> {
        let prepared = self.prepare(frame)?;
        self.output
            .try_encode_video(prepared.as_ref().unwrap_or(frame))
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.output.encode_audio(frame)
    }

    fn encode_processed_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.output.encode_processed_audio(frame)
    }

    fn handles_loudness(&self) -> bool {
        self.output.handles_loudness()
    }

    fn encode_live_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.output.encode_live_audio(frame)
    }

    fn encode_audio_with_gain_hold(&mut self, frame: &frame::Audio, hold: bool) -> Result<()> {
        self.output.encode_audio_with_gain_hold(frame, hold)
    }

    fn reset_after_skip(&mut self, video: i64, audio: i64) -> Result<bool> {
        self.output.reset_after_skip(video, audio)
    }

    fn set_video_end(&mut self, end: Option<i64>) -> Result<()> {
        self.output.set_video_end(end)
    }

    fn video_decoded(&mut self) -> Result<()> {
        self.output.video_decoded()
    }

    fn video_finished(&mut self) -> Result<()> {
        self.output.video_finished()
    }

    fn pad_audio(&mut self, samples: i64) -> Result<bool> {
        self.output.pad_audio(samples)
    }

    fn write_vtt_subtitles(
        &mut self,
        path: &str,
        start: i64,
        source: i64,
        end: Option<i64>,
    ) -> Result<()> {
        self.output.write_vtt_subtitles(path, start, source, end)
    }

    fn clear_vtt_subtitles(&mut self) -> Result<()> {
        self.output.clear_vtt_subtitles()
    }

    fn advance_vtt_subtitles(&mut self, position: i64) -> Result<()> {
        self.output.advance_vtt_subtitles(position)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    use ffmpeg_next::format::Pixel;

    use super::*;
    use crate::{graphics::logo_adapter::LogoOverlay, utils::config::LogoConfig};

    #[derive(Default)]
    struct Capture {
        logo: Option<Arc<LogoOverlay>>,
        frames: Vec<(Option<Arc<LogoOverlay>>, f64)>,
        opacity: f64,
        unmodified_header: usize,
    }

    impl FrameOutput for Capture {
        fn audio_frame_size(&self) -> usize {
            1024
        }

        fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
            self.unmodified_header = unsafe { frame.as_ptr() } as usize;
            self.frames.push((self.logo.clone(), self.opacity));
            Ok(())
        }

        fn encode_audio(&mut self, _: &frame::Audio) -> Result<()> {
            Ok(())
        }

        fn defer_text_overlays(&mut self, texts: &TextOverlays) -> bool {
            texts.iter().all(Option::is_none)
        }

        fn defer_shared_logo_overlay(&mut self, logo: &Arc<LogoOverlay>, opacity: f64) -> bool {
            self.logo = Some(logo.clone());
            self.opacity = opacity;

            true
        }

        fn clear_logo_overlay(&mut self) {
            self.logo = None;
        }
    }

    #[test]
    fn snapshots_survive_clip_boundaries_and_slow_consumers_without_changing_phase() {
        let mut config = OutputConfig::new(64, 64, 25, 48_000);
        config.logo = Some(LogoConfig {
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/media_images/animated-logo.gif")
                .to_string_lossy()
                .into_owned(),
            scale: None,
            opacity: 1.0,
            position: "0:0".into(),
        });
        let mut state = ProgramGraphicsState::default();
        let mut capture = Capture::default();
        let mut frame = frame::Video::new(Pixel::YUV420P, 64, 64);
        frame.set_pts(Some(0));
        {
            let mut output = ProgramGraphicsOutput::new(&mut capture, &mut state, &config);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                output.encode_video(&frame).unwrap();
                if output.output.logo.is_some() {
                    break;
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
        }
        assert_eq!(capture.unmodified_header, unsafe { frame.as_ptr() }
            as usize);
        let first = capture.logo.clone().unwrap();
        let first_pixels = first.0.frame.data(0).to_vec();
        // The next wrapper represents another file clip or a live resume.
        {
            let mut output = ProgramGraphicsOutput::new(&mut capture, &mut state, &config);
            frame.set_pts(Some(5)); // 200 ms falls in GIF frame 2 (100..400 ms).
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                output.set_logo_opacity(0.5);
                output.encode_video(&frame).unwrap();
                if output.output.logo.as_ref().unwrap().0.frame.data(0) != first_pixels {
                    break;
                }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            frame.set_pts(Some(15)); // 600 ms: the original first surface repeats.
            output.encode_video(&frame).unwrap();
        }
        assert!(Arc::ptr_eq(&first, capture.logo.as_ref().unwrap()));
        assert_eq!(
            first.0.frame.data(0),
            first_pixels,
            "queued consumers retain immutable source pixels"
        );
        assert_eq!(capture.frames.last().unwrap().1, 0.5);
        config.logo = None;
        ProgramGraphicsOutput::new(&mut capture, &mut state, &config)
            .encode_video(&frame)
            .unwrap();
        assert!(
            capture.logo.is_none(),
            "removing a source must clear its deferred surface"
        );
    }

    #[test]
    fn immediate_composition_blends_text_after_logo_and_keeps_source_unchanged() {
        use ff_graphics::{
            animation::TextPreparation,
            ffmpeg::text::TextOverlay,
            style::{TextPosition, TextStyle},
        };
        #[derive(Default)]
        struct Pixels(Vec<u8>);
        impl FrameOutput for Pixels {
            fn audio_frame_size(&self) -> usize {
                1024
            }
            fn encode_audio(&mut self, _: &frame::Audio) -> Result<()> {
                Ok(())
            }
            fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
                self.0 = frame.data(0).to_vec();
                Ok(())
            }
        }
        let mut config = OutputConfig::new(64, 64, 25, 48_000);
        config.logo = Some(LogoConfig {
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/media_images/animated-logo.gif")
                .to_string_lossy()
                .into_owned(),
            scale: Some("64:64".into()),
            opacity: 1.0,
            position: "0:0".into(),
        });
        let mut state = ProgramGraphicsState::default();
        state.configure(config.logo.as_ref(), 64, 64, None);
        let deadline = Instant::now() + Duration::from_secs(5);
        let logo = loop {
            if let Some(logo) = state.at(0) {
                break logo;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        };
        let text = TextOverlay::prepare(
            &TextStyle {
                font_size: 28.0,
                position_x: TextPosition::Pixels(8),
                position_y: TextPosition::Pixels(8),
                ..TextStyle::default()
            },
            "MMM",
            TextPreparation {
                canvas_width: 64,
                canvas_height: 64,
                frames_per_second: 25,
                timing: Default::default(),
            },
        )
        .unwrap()
        .unwrap()
        .snapshot(0, 0)
        .unwrap();
        let mut source = frame::Video::new(Pixel::YUV420P, 64, 64);
        for plane in 0..3 {
            source.data_mut(plane).fill(128);
        }
        source.set_pts(Some(0));
        let mut expected = source.clone();
        super::super::logo_adapter::blend_logo(&mut expected, &logo, 1.0);
        text.blend(&mut expected);
        let mut reversed = source.clone();
        text.blend(&mut reversed);
        super::super::logo_adapter::blend_logo(&mut reversed, &logo, 1.0);
        assert_ne!(expected.data(0), reversed.data(0));
        let mut pixels = Pixels::default();
        let mut output = ProgramGraphicsOutput::new(&mut pixels, &mut state, &config);
        output.set_text_overlays([Some(text), None]);
        output.encode_video(&source).unwrap();
        assert_eq!(pixels.0, expected.data(0));
        assert!(source.data(0).iter().all(|value| *value == 128));
    }

    #[test]
    fn removing_the_last_text_clears_deferred_output_metadata() {
        use ff_graphics::{
            animation::TextPreparation, ffmpeg::text::TextOverlay, style::TextStyle,
        };
        #[derive(Default)]
        struct Deferred {
            texts: TextOverlays,
            visible: Vec<bool>,
        }
        impl FrameOutput for Deferred {
            fn audio_frame_size(&self) -> usize {
                1024
            }
            fn encode_audio(&mut self, _: &frame::Audio) -> Result<()> {
                Ok(())
            }
            fn defer_text_overlays(&mut self, texts: &TextOverlays) -> bool {
                self.texts = texts.clone();
                true
            }
            fn encode_video(&mut self, _: &frame::Video) -> Result<()> {
                self.visible.push(self.texts.iter().any(Option::is_some));
                Ok(())
            }
        }
        let config = OutputConfig::new(64, 64, 25, 48000);
        let mut state = ProgramGraphicsState::default();
        let text = TextOverlay::prepare(
            &TextStyle::default(),
            "M",
            TextPreparation {
                canvas_width: 64,
                canvas_height: 64,
                frames_per_second: 25,
                timing: Default::default(),
            },
        )
        .unwrap()
        .unwrap()
        .snapshot(0, 0);
        let mut capture = Deferred::default();
        let frame = frame::Video::new(Pixel::YUV420P, 64, 64);
        let mut output = ProgramGraphicsOutput::new(&mut capture, &mut state, &config);
        output.set_text_overlays([text, None]);
        output.encode_video(&frame).unwrap();
        output.set_text_overlays(Default::default());
        output.encode_video(&frame).unwrap();
        assert_eq!(capture.visible, [true, false]);
    }

    #[test]
    fn no_logo_passes_the_original_frame_without_allocating_a_reference_header() {
        let config = OutputConfig::new(64, 64, 25, 48_000);
        let mut state = ProgramGraphicsState::default();
        let mut capture = Capture::default();
        let frame = frame::Video::new(Pixel::YUV420P, 64, 64);
        ProgramGraphicsOutput::new(&mut capture, &mut state, &config)
            .encode_video(&frame)
            .unwrap();
        assert_eq!(capture.unmodified_header, unsafe { frame.as_ptr() }
            as usize);
    }
}
