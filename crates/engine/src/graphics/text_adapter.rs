//! Engine adapter: resolve media-derived content before calling the graphics renderer.
use std::path::Path;

use anyhow::Result;
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

impl TextOverlay {
    pub fn prepare(
        config: &TextConfig,
        media_path: &str,
        context: TextPreparation,
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
        Ok(PreparedTextOverlay::prepare(&style, &text, context)?.map(Self))
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
