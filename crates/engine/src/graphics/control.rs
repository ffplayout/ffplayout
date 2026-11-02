//! Adapt native preparation results to the common controller at frame boundaries.
use std::sync::Arc;

use ff_graphics::{
    playback::Transition,
    scene::{Command, FrameLayer, LayerId, LayerSpec, ResourceStatus, SceneController, Scope},
    surface::{ColorEncoding, Resource, ResourceKey, SurfaceDescriptor},
};

use super::{
    clip::ClipSceneFrame,
    frame::{Content, GraphicsFrame},
    logo_adapter::LogoOverlay,
    text_adapter::{TextFrame, TextOverlays},
};

const LOGO: LayerId = LayerId(0);
const CLIP: LayerId = LayerId(1);
const TEXT: [LayerId; 2] = [LayerId(2), LayerId(3)];

struct PreparedSource<T> {
    key: ResourceKey,
    source: T,
}

pub(super) struct ReadyLogo {
    pub generation: u64,
    pub surface: Arc<LogoOverlay>,
}

pub(super) struct LogoUpdate {
    pub generation: Option<u64>,
    pub ready: Option<ReadyLogo>,
    pub failed: bool,
    pub opacity: f64,
}

pub(super) struct ProgramScene {
    controller: SceneController,
    revision: u64,
    last_time: Option<i64>,
    logo_request: Option<(u64, ResourceKey)>,
    logo: Option<PreparedSource<Arc<LogoOverlay>>>,
    logo_visible: bool,
    texts: [Option<PreparedSource<TextFrame>>; 2],
    failed_texts: [Option<PreparedSource<TextFrame>>; 2],
}

impl Default for ProgramScene {
    fn default() -> Self {
        // The native adapter is the sole owner. No queue, receipt or worker is
        // created for per-frame animation/scrolling updates.
        let controller = SceneController::new_local(3, 3);

        Self {
            controller,
            revision: 0,
            last_time: None,
            logo_request: None,
            logo: None,
            logo_visible: true,
            texts: Default::default(),
            failed_texts: Default::default(),
        }
    }
}

impl ProgramScene {
    fn prepare(&mut self, id: LayerId, now: i64) -> Option<ResourceKey> {
        self.revision = self.revision.checked_add(1)?;
        let key = ResourceKey {
            id: id.0,
            revision: self.revision,
        };
        let [ack] = self.controller.apply_at_frame(now, [Command::Prepare(key)]);

        ack.outcome.is_ok().then_some(key)
    }

    fn take(
        &mut self,
        key: ResourceKey,
        descriptor: SurfaceDescriptor,
        canvas: (u32, u32),
        replacing: bool,
        now: i64,
    ) -> bool {
        // Validate before clearing the previous active layer. A failed ready
        // result must never replace good pixels or stop media playback.
        if descriptor.validate().is_err() || canvas.0 == 0 || canvas.1 == 0 {
            self.controller.apply_at_frame(now, [Command::Failed(key)]);
            return false;
        }
        let ready = Command::Ready(Arc::new(Resource { key, descriptor }));
        let take = Command::Take {
            layer: LayerSpec {
                id: LayerId(key.id),
                resource: key,
                z: key.id as i32,
                scope: Scope::default(),
                canvas,
                position: (0, 0),
                opacity: 1.0,
            },
            // Compatibility adapters already evaluate their established
            // fade/scroll clocks. Applying another fade here would double it.
            transition: Transition::default(),
        };
        let outcome = if replacing {
            let [_, _, ack] = self
                .controller
                .apply_at_frame(now, [ready, Command::Clear(LayerId(key.id)), take]);
            ack.outcome
        } else {
            let [_, ack] = self.controller.apply_at_frame(now, [ready, take]);
            ack.outcome
        };

        outcome.is_ok()
    }

    fn release(&mut self, id: LayerId, key: ResourceKey, now: i64) {
        // A loading/failed replacement can have an older active layer with the
        // same ID. Clear it as well as the most recent preparation slot.
        self.controller
            .apply_at_frame(now, [Command::Clear(id), Command::Release(key)]);
    }

    pub(super) fn end_occurrence(&mut self) {
        let now = self.last_time.unwrap_or_default();
        for index in 0..TEXT.len() {
            self.clear_text(index, now);
        }
    }

    pub(super) fn frame(
        &mut self,
        now: i64,
        canvas: (u32, u32),
        logo: LogoUpdate,
        mut texts: TextOverlays,
        clip: Option<ClipSceneFrame>,
    ) -> GraphicsFrame {
        if self.last_time.is_some_and(|last| now < last) {
            self.controller.advance(now);
            self.logo_request = None;
            self.logo = None;
            self.texts = Default::default();
            self.failed_texts = Default::default();
        }
        self.last_time = Some(now);
        let logo_opacity = logo.opacity;
        self.update_logo(now, canvas, logo);
        for (index, text) in texts.iter().enumerate() {
            self.update_text(index, text.as_ref(), canvas, now);
        }

        let scene = self.controller.advance(now);
        let mut layers = std::array::from_fn(|_| None);
        layers[1] = clip.map(|clip| FrameLayer {
            id: CLIP,
            z: 1,
            content: Content::Clip(clip),
        });
        for evaluated in scene.evaluate(now, Scope::default()) {
            let content = match evaluated.id {
                LOGO => self
                    .logo
                    .as_ref()
                    .filter(|logo| {
                        logo.source.opacity() > 0
                            && logo.key == evaluated.content.layer.spec.resource
                    })
                    .map(|logo| Content::Logo {
                        surface: logo.source.clone(),
                        opacity: logo_opacity * evaluated.content.opacity,
                    }),
                id => TEXT.iter().position(|text| *text == id).and_then(|index| {
                    let active = self.texts[index]
                        .as_ref()
                        .filter(|active| active.key == evaluated.content.layer.spec.resource)?;
                    let surface = texts[index]
                        .take()
                        .filter(|surface| active.source.same_surface(surface))
                        .unwrap_or_else(|| active.source.clone());

                    Some(Content::Text {
                        surface: surface.with_opacity_factor(evaluated.content.opacity),
                        index,
                    })
                }),
            };
            if let Some(content) = content {
                layers[evaluated.id.0 as usize] = Some(FrameLayer {
                    id: evaluated.id,
                    z: evaluated.z,
                    content,
                });
            }
        }

        GraphicsFrame::from_layers(canvas, layers)
    }

    fn update_logo(&mut self, now: i64, canvas: (u32, u32), update: LogoUpdate) {
        let visible = update.opacity > 0.0;
        let Some(generation) = update.generation else {
            if let Some((_, key)) = self.logo_request.take() {
                self.release(LOGO, key, now);
            }
            self.logo = None;
            return;
        };
        if self.logo_request.map(|request| request.0) != Some(generation) {
            self.logo_request = self.prepare(LOGO, now).map(|key| (generation, key));
        }
        let Some((_, key)) = self.logo_request else {
            return;
        };
        if let Some(ready) = update.ready.filter(|ready| ready.generation == generation) {
            let source = ready.surface;
            if self.logo.as_ref().is_some_and(|active| active.key == key) {
                // Animation changes ready pixels, not the source generation or
                // presentation. Never enqueue control changes for each frame.
                self.logo = Some(PreparedSource { key, source });
            } else if self.controller.resource_status(key) == Some(ResourceStatus::Loading)
                && self.take(key, source.descriptor(), canvas, self.logo.is_some(), now)
            {
                self.logo = Some(PreparedSource { key, source });
                self.logo_visible = true;
            }
        } else if update.failed
            && self.controller.resource_status(key) == Some(ResourceStatus::Loading)
        {
            self.controller.apply_at_frame(now, [Command::Failed(key)]);
        }
        if self.logo.is_some() && visible != self.logo_visible {
            self.controller.apply_at_frame(
                now,
                [Command::Visibility {
                    layer: LOGO,
                    visible,
                }],
            );
            self.logo_visible = visible;
        }
    }

    fn clear_text(&mut self, index: usize, now: i64) {
        let active = self.texts[index].take();
        let latest = self.failed_texts[index].take().or(active);
        if let Some(source) = latest {
            self.release(TEXT[index], source.key, now);
        }
    }

    fn update_text(
        &mut self,
        index: usize,
        incoming: Option<&TextFrame>,
        canvas: (u32, u32),
        now: i64,
    ) {
        let id = TEXT[index];
        let Some(source) = incoming else {
            self.clear_text(index, now);
            return;
        };
        if self.texts[index]
            .as_mut()
            .is_some_and(|active| active.source.update_presentation(source))
        {
            if let Some(failed) = self.failed_texts[index].take() {
                self.controller
                    .apply_at_frame(now, [Command::Release(failed.key)]);
            }
            return;
        }
        if self.failed_texts[index]
            .as_ref()
            .is_some_and(|failed| failed.source.same_surface(source))
        {
            return;
        }
        if let Some(key) = self.prepare(id, now) {
            let prepared = PreparedSource {
                key,
                source: source.clone(),
            };
            if self.take(
                key,
                source.descriptor(ColorEncoding::Bt601Limited),
                canvas,
                self.texts[index].is_some(),
                now,
            ) {
                self.texts[index] = Some(prepared);
                self.failed_texts[index] = None;
            } else {
                self.failed_texts[index] = Some(prepared);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ff_graphics::{
        animation::{TextPreparation, TextTiming},
        ffmpeg::{logo::LogoOverlay as PreparedLogo, text::TextOverlay},
        style::{LogoStyle, TextPosition, TextScroll, TextStyle},
    };
    use ffmpeg_next::{format::Pixel, frame};

    use super::*;
    use crate::graphics::frame::GraphicsCache;

    const CANVAS: (u32, u32) = (128, 72);

    fn logo(value: u8) -> Arc<LogoOverlay> {
        let mut surface = frame::Video::new(Pixel::YUVA420P, 8, 4);
        for plane in 0..4 {
            surface
                .data_mut(plane)
                .fill(if plane == 0 { value } else { 128 });
        }
        let prepared = PreparedLogo::from_yuva(
            &LogoStyle {
                scale: None,
                opacity: 0.7,
                position: "2:2".into(),
            },
            surface,
            CANVAS.0,
            CANVAS.1,
        )
        .unwrap();

        Arc::new(LogoOverlay(Arc::new(prepared)))
    }

    fn update(generation: Option<u64>, source: Option<Arc<LogoOverlay>>) -> LogoUpdate {
        LogoUpdate {
            ready: generation
                .zip(source)
                .map(|(generation, surface)| ReadyLogo {
                    generation,
                    surface,
                }),
            generation,
            failed: false,
            opacity: 1.0,
        }
    }

    fn advance(
        scene: &mut ProgramScene,
        now: i64,
        logo: LogoUpdate,
        texts: TextOverlays,
    ) -> GraphicsFrame {
        scene.frame(now, CANVAS, logo, texts, None)
    }

    fn text(fade: bool) -> TextOverlay {
        TextOverlay::prepare(
            &TextStyle {
                font_size: 20.0,
                opacity: 0.6,
                position_x: TextPosition::Pixels(2),
                position_y: TextPosition::Pixels(2),
                scroll: TextScroll::RightToLeft {
                    pixels_per_second: 80,
                },
                fade_in_seconds: if fade { 1.0 } else { 0.0 },
                fade_out_seconds: if fade { 0.5 } else { 0.0 },
                ..Default::default()
            },
            "MMMM",
            TextPreparation {
                canvas_width: CANVAS.0,
                canvas_height: CANVAS.1,
                frames_per_second: 25,
                timing: TextTiming {
                    end_frame: fade.then_some(100),
                    ..Default::default()
                },
            },
        )
        .unwrap()
        .unwrap()
    }

    fn pixels(snapshot: &GraphicsFrame, size: (u32, u32)) -> Vec<u8> {
        let mut target = frame::Video::new(Pixel::YUV420P, size.0, size.1);
        for plane in 0..3 {
            target.data_mut(plane).fill(128);
        }
        snapshot
            .blend(&mut target, None, &mut GraphicsCache::default())
            .unwrap();
        let mut result = Vec::new();
        for plane in 0..3 {
            for row in 0..target.plane_height(plane) as usize {
                let start = row * target.stride(plane);
                result.extend_from_slice(
                    &target.data(plane)[start..start + target.plane_width(plane) as usize],
                );
            }
        }

        result
    }

    #[test]
    fn logo_loading_failure_stale_results_and_clear_preserve_the_correct_active_source() {
        let mut scene = ProgramScene::default();
        assert!(advance(&mut scene, 0, update(Some(1), None), Default::default()).is_empty());
        let first_key = scene.logo_request.unwrap().1;
        assert_eq!(
            scene.controller.resource_status(first_key),
            Some(ResourceStatus::Loading)
        );
        let first = logo(200);
        let queued = advance(
            &mut scene,
            1,
            update(Some(1), Some(first.clone())),
            Default::default(),
        );
        assert_eq!(
            scene.controller.resource_status(first_key),
            Some(ResourceStatus::Active)
        );
        let loading = advance(&mut scene, 2, update(Some(2), None), Default::default());
        let replacement = scene.logo_request.unwrap().1;
        assert!(Arc::ptr_eq(loading.logo().unwrap().0, &first));
        assert_eq!(
            scene.controller.resource_status(first_key),
            Some(ResourceStatus::Active)
        );
        assert_eq!(
            scene.controller.resource_status(replacement),
            Some(ResourceStatus::Loading)
        );
        let mut stale = update(Some(2), None);
        stale.ready = Some(ReadyLogo {
            generation: 1,
            surface: logo(50),
        });
        let unchanged = advance(&mut scene, 3, stale, Default::default());
        assert!(Arc::ptr_eq(unchanged.logo().unwrap().0, &first));
        assert_eq!(
            scene.controller.resource_status(replacement),
            Some(ResourceStatus::Loading)
        );
        let mut failed = update(Some(2), None);
        failed.failed = true;
        assert!(!advance(&mut scene, 4, failed, Default::default()).is_empty());
        assert_eq!(
            scene.controller.resource_status(replacement),
            Some(ResourceStatus::Failed)
        );
        let mut late = update(None, None);
        late.ready = Some(ReadyLogo {
            generation: 2,
            surface: logo(50),
        });
        assert!(advance(&mut scene, 5, late, Default::default()).is_empty());
        assert_eq!(scene.controller.resource_status(first_key), None);
        assert_eq!(scene.controller.resource_status(replacement), None);
        assert_eq!(pixels(&queued, CANVAS), pixels(&loading, CANVAS));
        assert!(
            !advance(
                &mut scene,
                6,
                update(Some(3), Some(logo(50))),
                Default::default()
            )
            .is_empty()
        );
    }

    #[test]
    fn animation_and_text_scrolling_do_not_allocate_new_controller_revisions() {
        let mut scene = ProgramScene::default();
        let first_logo = logo(200);
        let second_logo = logo(50);
        let text = text(false);
        advance(
            &mut scene,
            0,
            update(Some(1), Some(first_logo.clone())),
            [text.snapshot(0, 0), None],
        );
        let first = scene.controller.advance(0);
        let preparation = scene.revision;
        for frame in 1..500 {
            let now = frame * 40_000;
            let source = if frame % 3 == 0 {
                first_logo.clone()
            } else {
                second_logo.clone()
            };
            advance(
                &mut scene,
                now,
                update(Some(1), Some(source)),
                [text.snapshot(frame, frame), None],
            );
            let current = scene.controller.advance(now);
            assert_eq!(scene.revision, preparation);
            assert!(Arc::ptr_eq(&first.layers, &current.layers));
        }
    }

    #[test]
    fn native_fades_scroll_clip_graphics_and_clock_rewinds_keep_legacy_pixels() {
        let mut scene = ProgramScene::default();
        let logo = logo(200);
        let static_text = text(true);
        let runtime_text = text(false);
        let clip = crate::graphics::clip::tests::fixture(CANVAS)
            .frame_at(120_000)
            .unwrap();
        for position in [0, 5, 20, 35, 99, 100, 20] {
            let texts = [
                static_text.snapshot(position, position),
                runtime_text.snapshot(position, position),
            ];
            let mut input = update(Some(1), Some(logo.clone()));
            input.opacity = 0.5;
            let actual = scene.frame(
                position * 40_000,
                CANVAS,
                input,
                texts.clone(),
                Some(clip.clone()),
            );
            let expected =
                GraphicsFrame::new(CANVAS, Some(logo.clone()), 0.5, texts, Some(clip.clone()));
            for size in [CANVAS, (64, 36)] {
                assert_eq!(
                    pixels(&actual, size),
                    pixels(&expected, size),
                    "frame={position}, size={size:?}"
                );
            }
        }
    }

    #[test]
    fn occurrence_end_releases_text_and_hidden_logo_resumes_without_repreparing() {
        let mut scene = ProgramScene::default();
        let logo = logo(200);
        let text = text(false);
        let queued = advance(
            &mut scene,
            0,
            update(Some(1), Some(logo.clone())),
            [text.snapshot(25, 25), None],
        );
        let text_key = scene.texts[0].as_ref().unwrap().key;
        let logo_key = scene.logo.as_ref().unwrap().key;
        scene.end_occurrence();
        assert_eq!(scene.controller.resource_status(text_key), None);
        assert_eq!(
            scene.controller.resource_status(logo_key),
            Some(ResourceStatus::Active)
        );
        let logo_only = advance(
            &mut scene,
            1,
            update(Some(1), Some(logo.clone())),
            Default::default(),
        );
        assert!(
            pixels(&queued, CANVAS) != pixels(&logo_only, CANVAS),
            "queued text must survive the occurrence end"
        );
        let preparation = scene.revision;
        let mut hidden = update(Some(1), Some(logo.clone()));
        hidden.opacity = 0.0;
        assert!(advance(&mut scene, 2, hidden, Default::default()).is_empty());
        let visible = advance(
            &mut scene,
            3,
            update(Some(1), Some(logo)),
            Default::default(),
        );
        assert!(!visible.is_empty());
        assert_eq!(scene.revision, preparation);
    }

    #[test]
    fn rejected_text_cannot_bypass_admission_and_clear_releases_pending_and_active_resources() {
        let mut scene = ProgramScene::default();
        let original = text(false);
        let replacement = text(false);
        advance(
            &mut scene,
            0,
            update(None, None),
            [original.snapshot(25, 25), None],
        );
        let last_good = advance(
            &mut scene,
            1,
            update(None, None),
            [original.snapshot(30, 30), None],
        );
        let old_key = scene.texts[0].as_ref().unwrap().key;
        let candidate = replacement.snapshot(50, 50).unwrap();
        let pending = scene.prepare(TEXT[0], 2).unwrap();
        let mut invalid = candidate.descriptor(ColorEncoding::Bt601Limited);
        invalid.storage_bytes = 0;
        assert!(!scene.take(pending, invalid, CANVAS, true, 2));
        scene.failed_texts[0] = Some(PreparedSource {
            key: pending,
            source: candidate.clone(),
        });
        let revision = scene.revision;
        let held = advance(&mut scene, 3, update(None, None), [Some(candidate), None]);
        assert!(pixels(&held, CANVAS) == pixels(&last_good, CANVAS));
        assert_eq!(
            scene.revision, revision,
            "do not retry the same rejected preparation each frame"
        );
        assert_eq!(
            scene.controller.resource_status(old_key),
            Some(ResourceStatus::Active)
        );
        assert_eq!(
            scene.controller.resource_status(pending),
            Some(ResourceStatus::Failed)
        );
        scene.end_occurrence();
        assert_eq!(scene.controller.resource_status(old_key), None);
        assert_eq!(scene.controller.resource_status(pending), None);
        assert!(scene.controller.advance(3).layers.is_empty());
    }

    #[test]
    fn request_exhaustion_keeps_the_last_valid_text_pose_instead_of_taking_new_pixels() {
        let mut scene = ProgramScene::default();
        let original = text(false);
        let replacement = text(false);
        let expected = advance(
            &mut scene,
            0,
            update(None, None),
            [original.snapshot(30, 30), None],
        );
        scene.revision = u64::MAX;
        let held = advance(
            &mut scene,
            1,
            update(None, None),
            [replacement.snapshot(50, 50), None],
        );
        assert!(pixels(&held, CANVAS) == pixels(&expected, CANVAS));
    }

    #[test]
    fn replacements_release_old_resources_and_channel_controllers_are_isolated() {
        let mut first = ProgramScene::default();
        let mut other = ProgramScene::default();
        let first_pixels = logo(200);
        let other_pixels = logo(50);
        let other_frame = advance(
            &mut other,
            0,
            update(Some(1), Some(other_pixels)),
            Default::default(),
        );
        let other_key = other.logo.as_ref().unwrap().key;
        let mut old = None;
        for generation in 1..100 {
            advance(
                &mut first,
                generation as i64,
                update(Some(generation), Some(first_pixels.clone())),
                Default::default(),
            );
            if let Some(key) = old {
                assert_eq!(first.controller.resource_status(key), None);
            }
            old = Some(first.logo.as_ref().unwrap().key);
            assert_eq!(first.controller.advance(generation as i64).layers.len(), 1);
        }
        assert!(advance(&mut first, 100, update(None, None), Default::default()).is_empty());
        assert_eq!(
            other.controller.resource_status(other_key),
            Some(ResourceStatus::Active)
        );
        assert!(!other_frame.is_empty());
    }
}
