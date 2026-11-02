//! One immutable graphics snapshot for immediate composition and queued outputs.
use std::sync::Arc;

use anyhow::Result;
#[cfg(test)]
use ff_graphics::scene::LayerId;
use ff_graphics::{
    ffmpeg::text::TextScaleCache,
    scene::{FrameLayer, FrameLayers},
};
use ffmpeg_next::frame;

use super::{
    clip::ClipSceneFrame,
    logo_adapter::{LogoOverlay, blend_logo},
    text_adapter::{TextFrame, measure_text},
};

#[cfg(test)]
use super::text_adapter::TextOverlays;

#[derive(Clone)]
pub(super) enum Content {
    Logo {
        surface: Arc<LogoOverlay>,
        opacity: f64,
    },
    Clip(ClipSceneFrame),
    Text {
        surface: TextFrame,
        index: usize,
    },
}

/// Legacy logo/clip/text ordering is an explicit group layout. Clip layers keep
/// their local z-order and IDs; they cannot collide with the program layer IDs.
/// Cloning retains only ready surfaces, never a decoder or preparation worker.
#[derive(Clone, Default)]
pub(crate) struct GraphicsFrame {
    layers: FrameLayers<Content, 4>,
    canvas: (u32, u32),
}

#[derive(Default)]
pub(crate) struct GraphicsCache {
    source_logo: Option<Arc<LogoOverlay>>,
    logo: Option<LogoOverlay>,
    texts: [TextScaleCache; 2],
    text_failed: [bool; 2],
    logo_canvas: (u32, u32),
    logo_position: Option<String>,
}

impl GraphicsCache {
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

impl GraphicsFrame {
    pub(super) fn from_layers(
        canvas: (u32, u32),
        layers: [Option<FrameLayer<Content>>; 4],
    ) -> Self {
        Self {
            canvas,
            layers: FrameLayers::new(layers),
        }
    }

    /// Reference construction for compatibility tests; production uses the controller.
    #[cfg(test)]
    pub(crate) fn new(
        canvas: (u32, u32),
        logo: Option<Arc<LogoOverlay>>,
        opacity: f64,
        texts: TextOverlays,
        clip: Option<ClipSceneFrame>,
    ) -> Self {
        let logo = logo
            .filter(|logo| opacity > 0.0 && logo.opacity() > 0)
            .map(|surface| Content::Logo { surface, opacity });
        let [static_text, runtime_text] = texts;
        let contents = [
            logo,
            clip.map(Content::Clip),
            static_text.map(|surface| Content::Text { surface, index: 0 }),
            runtime_text.map(|surface| Content::Text { surface, index: 1 }),
        ];

        let mut index = 0;
        let layers = contents.map(|content| {
            let id = index;
            index += 1;

            content.map(|content| FrameLayer {
                id: LayerId(id),
                z: id as i32,
                content,
            })
        });

        Self {
            canvas,
            layers: FrameLayers::new(layers),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    /// The caller owns a writable target and one cache per destination size.
    /// Logos keep destination-pixel dimensions; text/clip graphics use their
    /// reference canvas. HLS composes its largest rendition before downscaling.
    pub(crate) fn blend(
        &self,
        target: &mut frame::Video,
        logo_position: Option<&str>,
        cache: &mut GraphicsCache,
    ) -> Result<()> {
        let mut has_logo = false;
        let mut has_text = [false; 2];
        for layer in self.layers.layers() {
            match &layer.content {
                Content::Logo { surface, opacity } => {
                    has_logo = true;
                    if !cache
                        .source_logo
                        .as_ref()
                        .is_some_and(|previous| Arc::ptr_eq(previous, surface))
                        || cache.logo_canvas != (target.width(), target.height())
                        || cache.logo_position.as_deref() != logo_position
                    {
                        cache.logo = Some(surface.positioned(
                            logo_position,
                            target.width(),
                            target.height(),
                        )?);
                        cache.source_logo = Some(surface.clone());
                        cache.logo_canvas = (target.width(), target.height());
                        cache.logo_position = logo_position.map(str::to_owned);
                    }
                    if let Some(logo) = &cache.logo {
                        crate::benchmark::measure_overlay(
                            crate::benchmark::Stage::LogoOverlay,
                            logo.width(),
                            logo.height(),
                            || blend_logo(target, logo, *opacity),
                        );
                    }
                }
                Content::Clip(clip) => clip.blend(target),
                Content::Text { surface, index } => {
                    has_text[*index] = true;
                    let result = measure_text(*index, surface, || {
                        surface.blend_scaled(target, self.canvas, &mut cache.texts[*index])
                    });
                    if let Err(error) = result {
                        if !cache.text_failed[*index] {
                            log::warn!("Text graphics skipped at destination size: {error:#}");
                        }
                        cache.text_failed[*index] = true;
                    } else {
                        cache.text_failed[*index] = false;
                    }
                }
            }
        }

        // Caches must not keep replaced/cleared graphics alive indefinitely.
        if !has_logo {
            cache.source_logo = None;
            cache.logo = None;
        }
        for (index, (present, text)) in has_text.into_iter().zip(&mut cache.texts).enumerate() {
            if !present {
                *text = TextScaleCache::default();
                cache.text_failed[index] = false;
            }
        }

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn logo(&self) -> Option<(&Arc<LogoOverlay>, f64)> {
        self.layers.layers().find_map(|layer| match &layer.content {
            Content::Logo { surface, opacity } => Some((surface, *opacity)),
            _ => None,
        })
    }

    #[cfg(test)]
    pub(crate) fn clip(&self) -> Option<&ClipSceneFrame> {
        self.layers.layers().find_map(|layer| match &layer.content {
            Content::Clip(clip) => Some(clip),
            _ => None,
        })
    }
}

#[cfg(test)]
impl GraphicsCache {
    pub(crate) fn logo(&self) -> Option<&LogoOverlay> {
        self.logo.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use ff_graphics::{
        animation::TextPreparation,
        ffmpeg::text::TextOverlay,
        style::{RgbaColor, TextPosition, TextStyle},
    };
    use ffmpeg_next::format::Pixel;

    use super::*;
    use crate::utils::config::LogoConfig;

    fn background(width: u32, height: u32) -> frame::Video {
        let mut result = frame::Video::new(Pixel::YUV420P, width, height);
        for plane in 0..3 {
            result.data_mut(plane).fill(128);
        }

        result
    }

    fn logo() -> Arc<LogoOverlay> {
        Arc::new(
            LogoOverlay::load(
                &LogoConfig {
                    path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../tests_assets/storage/media_images/animated-logo.gif")
                        .to_string_lossy()
                        .into_owned(),
                    scale: Some("64:32".into()),
                    opacity: 0.7,
                    position: "0:0".into(),
                },
                320,
                180,
            )
            .unwrap(),
        )
    }

    #[test]
    fn unified_layers_match_legacy_pixels_at_native_and_scaled_resolutions() {
        let logo = logo();
        let clip = crate::graphics::clip::tests::fixture((320, 180))
            .frame_at(120_000)
            .unwrap();
        let texts = [0, 1].map(|index| {
            TextOverlay::prepare(
                &TextStyle {
                    font_size: 28.0,
                    opacity: 0.6,
                    position_x: TextPosition::Pixels(2 + index * 8),
                    position_y: TextPosition::Pixels(2),
                    text_color: RgbaColor {
                        r: 30 + index as u8 * 200,
                        g: 10,
                        b: 100,
                        a: 200,
                    },
                    ..Default::default()
                },
                "MMMM",
                TextPreparation {
                    canvas_width: 320,
                    canvas_height: 180,
                    frames_per_second: 25,
                    timing: Default::default(),
                },
            )
            .unwrap()
            .unwrap()
            .snapshot(0, 0)
        });
        let snapshot = GraphicsFrame::new(
            (320, 180),
            Some(logo.clone()),
            0.5,
            texts.clone(),
            Some(clip.clone()),
        );
        for (width, height) in [(320, 180), (160, 90)] {
            let mut cache = GraphicsCache::default();
            for _ in 0..2 {
                let mut actual = background(width, height);
                let mut expected = background(width, height);
                snapshot
                    .blend(&mut actual, Some("0:0"), &mut cache)
                    .unwrap();
                let positioned = logo.positioned(Some("0:0"), width, height).unwrap();
                blend_logo(&mut expected, &positioned, 0.5);
                clip.blend(&mut expected);
                for text in texts.iter().flatten() {
                    text.blend_scaled(&mut expected, (320, 180), &mut TextScaleCache::default())
                        .unwrap();
                }
                for plane in 0..3 {
                    for row in 0..actual.plane_height(plane) as usize {
                        let columns = actual.plane_width(plane) as usize;
                        assert_eq!(
                            &actual.data(plane)
                                [row * actual.stride(plane)..row * actual.stride(plane) + columns],
                            &expected.data(plane)[row * expected.stride(plane)
                                ..row * expected.stride(plane) + columns],
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn exhausted_scaled_text_budget_skips_the_layer_without_failing_video() {
        let budget = ff_graphics::preparation::ResourceBudget::new(4 * 1024 * 1024, 8, 1);
        let text = TextOverlay::prepare_budgeted(
            &TextStyle::default(),
            "MMM",
            TextPreparation {
                canvas_width: 320,
                canvas_height: 180,
                frames_per_second: 25,
                timing: Default::default(),
            },
            Some(&budget),
        )
        .unwrap()
        .unwrap();
        let frame = GraphicsFrame::new((320, 180), None, 0.0, [text.snapshot(0, 0), None], None);
        let used = budget.usage().0;
        let blocker = budget.reserve(4 * 1024 * 1024 - used).unwrap();
        let mut target = background(160, 90);
        let mut cache = GraphicsCache::default();
        frame.blend(&mut target, None, &mut cache).unwrap();
        assert!(target.data(0).iter().all(|value| *value == 128));
        assert!(cache.text_failed[0]);
        drop(blocker);
        frame.blend(&mut target, None, &mut cache).unwrap();
        assert!(!cache.text_failed[0]);
        assert!(target.data(0).iter().any(|value| *value != 128));
    }

    #[test]
    fn cached_logo_layout_tracks_destination_and_clear_releases_surface() {
        let source = logo();
        let weak = Arc::downgrade(&source);
        let snapshot = GraphicsFrame::new((320, 180), Some(source), 1.0, Default::default(), None);
        let queued = snapshot.clone();
        let mut cache = GraphicsCache::default();
        for (width, height) in [(320, 180), (160, 90)] {
            snapshot
                .blend(
                    &mut background(width, height),
                    Some("W-w-8:H-h-8"),
                    &mut cache,
                )
                .unwrap();
            assert_eq!(
                cache.logo().unwrap().position(),
                (width - 64 - 8, height - 32 - 8)
            );
        }
        snapshot
            .blend(&mut background(160, 90), Some("0:0"), &mut cache)
            .unwrap();
        assert_eq!(cache.logo().unwrap().position(), (0, 0));
        drop(snapshot);
        GraphicsFrame::default()
            .blend(&mut background(160, 90), None, &mut cache)
            .unwrap();
        assert!(cache.logo().is_none());
        assert!(
            weak.upgrade().is_some(),
            "a queued frame still owns its resource"
        );
        drop(queued);
        assert!(weak.upgrade().is_none());
    }
}
