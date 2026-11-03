//! CPU scene adapter: reuses the production YUVA kernels, not another blend policy.
use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Result, ensure};
use ffmpeg_next::{format::Pixel, frame};

use crate::{
    bitmap::StraightRgbaBitmap,
    ffmpeg::sources::animation::PreparedSurface,
    ffmpeg::text::rgba_to_yuva420p,
    preparation::{PreparationService, PreparationWeak, ResourceBudget},
    scene::{LayerId, LayerSpec, SceneRevision, Scope},
    surface::{ColorEncoding, Resource, ResourceKey, SurfaceStore},
};

use super::{
    metadata::{ColorPolicy, color_properties, yuva_descriptor},
    overlay::{OverlayFrame, alpha_bounds, chroma_alpha, prepare_execution},
    surface::{SurfaceFrame, SurfaceScaleCache},
};

pub struct CpuScene {
    pub(in crate::ffmpeg) surfaces: SurfaceStore<PreparedSurface>,
    color_policy: ColorPolicy,
    pub(in crate::ffmpeg) preparation: Option<PreparationWeak>,
    pub(in crate::ffmpeg) budget: Option<ResourceBudget>,
}

/// One cache per output resolution. Unchanged surfaces are scaled only once;
/// entries disappear as soon as their layer leaves the selected scene.
pub struct SceneScaleCache {
    layers: BTreeMap<LayerId, SurfaceScaleCache>,
    max_bytes: usize,
}

impl Default for SceneScaleCache {
    fn default() -> Self {
        Self::new(64 * 1024 * 1024)
    }
}

impl SceneScaleCache {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            layers: BTreeMap::new(),
            max_bytes,
        }
    }
    pub fn clear(&mut self) {
        self.layers.clear();
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CompositionReport {
    pub blended: usize,
    pub missing: usize,
    pub unsupported: usize,
}

impl CpuScene {
    pub fn new(max_surfaces: usize, max_bytes: usize) -> Self {
        Self {
            surfaces: SurfaceStore::new(max_surfaces, max_bytes),
            color_policy: ColorPolicy::LegacySignal,
            preparation: None,
            budget: None,
        }
    }

    pub fn with_color_policy(mut self, policy: ColorPolicy) -> Self {
        self.color_policy = policy;

        self
    }

    pub fn with_preparation(mut self, service: &PreparationService) -> Self {
        self.budget = Some(service.budget());
        self.preparation = Some(service.downgrade());

        self
    }

    pub fn with_budget(mut self, budget: ResourceBudget) -> Self {
        self.budget = Some(budget);

        self
    }

    /// Prepare once outside frame composition. Matches existing native text
    /// conversion: straight alpha, coverage-weighted chroma, BT.601 limited.
    pub fn prepare_bitmap(
        &mut self,
        key: ResourceKey,
        bitmap: &StraightRgbaBitmap,
    ) -> Result<Arc<Resource>> {
        ensure!(
            bitmap.width > 0 && bitmap.height > 0 && bitmap.width <= 8192 && bitmap.height <= 8192,
            "bitmap dimensions are unsupported"
        );
        ensure!(
            bitmap.width.is_multiple_of(2) && bitmap.height.is_multiple_of(2),
            "CPU bitmap dimensions must be even"
        );
        ensure!(
            bitmap.pixels.len() == bitmap.width as usize * bitmap.height as usize * 4,
            "bitmap storage does not match dimensions"
        );
        let frame = rgba_to_yuva420p(bitmap)?;

        self.prepare_yuva(key, frame, ColorEncoding::Bt601Limited)
    }

    /// Accept a prepared decoder/logo result without copying its pixels.
    /// Actual strides/storage are accounted for, including chroma coverage.
    pub fn prepare_yuva(
        &mut self,
        key: ResourceKey,
        frame: frame::Video,
        color: ColorEncoding,
    ) -> Result<Arc<Resource>> {
        ensure!(
            frame.format() == Pixel::YUVA420P,
            "CPU scene requires YUVA420P surfaces"
        );
        ensure!(
            frame.width() > 0
                && frame.height() > 0
                && frame.width() <= 8192
                && frame.height() <= 8192,
            "surface dimensions are unsupported"
        );
        ensure!(
            frame.width().is_multiple_of(2) && frame.height().is_multiple_of(2),
            "CPU surface dimensions must be even"
        );
        ensure!(
            frame.planes() == 4,
            "surface must have four populated planes"
        );
        for plane in 0..4 {
            let divisor = if plane == 1 || plane == 2 { 2 } else { 1 };
            let width = frame.width() as usize / divisor;
            let height = frame.height() as usize / divisor;
            ensure!(
                frame.stride(plane) >= width
                    && frame.data(plane).len() >= frame.stride(plane).saturating_mul(height),
                "surface plane storage does not match dimensions"
            );
        }
        let (width, height) = (frame.width(), frame.height());
        let alpha = chroma_alpha(&frame, width, height);
        let bounds = alpha_bounds(&frame);
        let descriptor = yuva_descriptor(&frame, bounds, alpha.len(), color)?;
        prepare_execution(bounds);
        let lease = self
            .budget
            .as_ref()
            .map(|budget| budget.reserve(descriptor.storage_bytes))
            .transpose()?;
        let surface = Arc::new(OverlayFrame {
            lease,
            frame,
            bounds,
            chroma_alpha: alpha,
            x: 0,
            y: 0,
            width,
            height,
            opacity: 255,
        });

        self.surfaces
            .insert(key, descriptor, Arc::new(PreparedSurface::still(surface)))
    }

    /// Select ready pixels on the program thread, before lookahead/fan-out.
    /// The fixed layer container allocates no storage per frame.
    pub fn freeze(&self, scene: &SceneRevision, program_us: i64, scope: Scope) -> FrozenScene {
        for source in self.surfaces.values() {
            source.begin_frame(program_us);
        }
        let mut frozen = FrozenScene {
            layers: std::array::from_fn(|_| None),
            report: CompositionReport::default(),
            policy: self.color_policy,
        };
        for (index, evaluated) in scene.evaluate(program_us, scope).enumerate() {
            if index >= frozen.layers.len() {
                frozen.report.unsupported += 1;
                continue;
            }
            let evaluated = evaluated.content;
            let layer = evaluated.layer;
            let Some(source) = self.surfaces.get(&layer.resource) else {
                frozen.report.missing += 1;
                continue;
            };
            let elapsed = layer.presentation.elapsed_us(program_us);
            let Some(surface) = source.select(
                elapsed,
                (scope.0, layer.spec.id.0, program_us.saturating_sub(elapsed)),
            ) else {
                continue;
            };
            frozen.layers[index] = Some(FrozenLayer {
                spec: layer.spec,
                color: layer.resource.descriptor.color,
                frame: SurfaceFrame {
                    surface,
                    x: layer.spec.position.0 & !1,
                    y: layer.spec.position.1 & !1,
                    opacity: evaluated.opacity,
                },
            });
        }

        for source in self.surfaces.values() {
            source.end_frame();
        }

        frozen
    }

    pub fn suspend_streams(&self) {
        for source in self.surfaces.values() {
            source.suspend();
        }
    }

    pub fn stop_streams(&self) {
        for source in self.surfaces.values() {
            source.stop();
        }
    }

    pub fn blend(
        &self,
        scene: &SceneRevision,
        program_us: i64,
        scope: Scope,
        target: &mut frame::Video,
        color: ColorEncoding,
    ) -> CompositionReport {
        self.freeze(scene, program_us, scope).blend(target, color)
    }

    pub fn retire(&mut self) {
        self.surfaces.retire();
    }

    pub fn blend_scaled(
        &self,
        scene: &SceneRevision,
        position_us: i64,
        scope: Scope,
        target: &mut frame::Video,
        color: ColorEncoding,
        cache: &mut SceneScaleCache,
    ) -> CompositionReport {
        self.freeze(scene, position_us, scope)
            .blend_scaled(target, color, cache)
    }
}

#[derive(Clone)]
struct FrozenLayer {
    spec: LayerSpec,
    color: ColorEncoding,
    frame: SurfaceFrame,
}

/// Immutable program-frame selection. Output queues retain pixels only; no
/// decoder requests or source-clock changes occur while an encoder consumes it.
#[derive(Clone)]
pub struct FrozenScene {
    layers: [Option<FrozenLayer>; 32],
    report: CompositionReport,
    policy: ColorPolicy,
}

impl FrozenScene {
    /// Release old pixel leases when the caller recycles unreferenced metadata.
    pub fn clear(&mut self) {
        for layer in &mut self.layers {
            *layer = None;
        }
        self.report = CompositionReport::default();
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(Option::is_none)
    }

    pub fn matches_canvas(&self, canvas: (u32, u32)) -> bool {
        self.layers
            .iter()
            .flatten()
            .all(|layer| layer.spec.canvas == canvas)
    }

    fn supported(&self, layer: &FrozenLayer, target: &frame::Video, color: ColorEncoding) -> bool {
        target.format() == Pixel::YUV420P
            && layer.color == color
            && (self.policy == ColorPolicy::LegacySignal
                || self.policy.supports(
                    color_properties(&layer.frame.surface.frame),
                    color_properties(target),
                ))
    }

    pub fn blend(&self, target: &mut frame::Video, color: ColorEncoding) -> CompositionReport {
        let mut report = self.report;
        for layer in self.layers.iter().flatten() {
            if !self.supported(layer, target, color)
                || layer.spec.canvas != (target.width(), target.height())
            {
                report.unsupported += 1;
                continue;
            }
            layer.frame.blend(target);
            report.blended += 1;
        }

        report
    }

    pub fn blend_scaled(
        &self,
        target: &mut frame::Video,
        color: ColorEncoding,
        cache: &mut SceneScaleCache,
    ) -> CompositionReport {
        cache.layers.retain(|id, _| {
            self.layers
                .iter()
                .flatten()
                .any(|layer| layer.spec.id == *id)
        });
        let mut cache_bytes: usize = cache
            .layers
            .values()
            .map(SurfaceScaleCache::storage_bytes)
            .sum();
        let mut report = self.report;
        for layer in self.layers.iter().flatten() {
            if !self.supported(layer, target, color) {
                report.unsupported += 1;
                continue;
            }
            let scaled = cache.layers.entry(layer.spec.id).or_default();
            cache_bytes -= scaled.storage_bytes();
            scaled.set_limits(cache.max_bytes.saturating_sub(cache_bytes), 8192);
            let result = layer.frame.blend_scaled(target, layer.spec.canvas, scaled);
            cache_bytes += scaled.storage_bytes();
            if result.is_err() {
                report.unsupported += 1;
            } else {
                report.blended += 1;
            }
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::super::overlay::blend_overlay;
    use super::*;
    use crate::{
        playback::Transition,
        scene::{Command, LayerId, LayerSpec, SceneController},
    };

    fn bitmap(rgba: [u8; 4]) -> StraightRgbaBitmap {
        StraightRgbaBitmap {
            width: 4,
            height: 4,
            pixels: rgba.repeat(16),
        }
    }

    fn background() -> frame::Video {
        let mut result = frame::Video::new(Pixel::YUV420P, 8, 8);
        result.data_mut(0).fill(16);
        result.data_mut(1).fill(128);
        result.data_mut(2).fill(128);

        result
    }

    #[test]
    fn scaling_cache_budget_skips_only_the_graphic_and_reuses_unchanged_surfaces() {
        use crate::playlist::{Action, ClipSchedule, Event, Limits};

        let mut cpu = CpuScene::new(1, 128 * 1024);
        let resource = cpu
            .prepare_bitmap(ResourceKey { id: 1, revision: 1 }, &bitmap([255; 4]))
            .unwrap();
        let schedule = ClipSchedule::compile(
            100,
            &[Event {
                at_us: 0,
                action: Action::Take {
                    layer: LayerSpec {
                        id: LayerId(1),
                        resource: resource.key,
                        z: 0,
                        scope: Scope::default(),
                        canvas: (16, 16),
                        position: (0, 0),
                        opacity: 1.0,
                    },
                    transition: Transition::default(),
                },
            }],
            &[resource],
            Limits::default(),
        )
        .unwrap();
        let selected = schedule.frame_at(Some(0)).unwrap();
        let mut actual = background();
        let before = actual.data(0).to_vec();
        let mut small = SceneScaleCache::new(1);
        let report = cpu.blend_scaled(
            selected.scene,
            0,
            Scope::default(),
            &mut actual,
            ColorEncoding::Bt601Limited,
            &mut small,
        );
        assert_eq!((report.blended, report.unsupported), (0, 1));
        assert_eq!(actual.data(0), before);
        assert_eq!(
            small
                .layers
                .values()
                .map(SurfaceScaleCache::storage_bytes)
                .sum::<usize>(),
            0
        );
        let mut cache = SceneScaleCache::default();
        assert_eq!(
            cpu.blend_scaled(
                selected.scene,
                0,
                Scope::default(),
                &mut actual,
                ColorEncoding::Bt601Limited,
                &mut cache
            )
            .blended,
            1
        );
        let bytes = cache
            .layers
            .values()
            .map(SurfaceScaleCache::storage_bytes)
            .sum::<usize>();
        assert!(bytes > 0);
        cpu.blend_scaled(
            selected.scene,
            1,
            Scope::default(),
            &mut actual,
            ColorEncoding::Bt601Limited,
            &mut cache,
        );
        assert_eq!(
            bytes,
            cache
                .layers
                .values()
                .map(SurfaceScaleCache::storage_bytes)
                .sum::<usize>()
        );
        cpu.blend_scaled(
            &SceneRevision::default(),
            1,
            Scope::default(),
            &mut actual,
            ColorEncoding::Bt601Limited,
            &mut cache,
        );
        assert!(cache.layers.is_empty());
        let mut oversized = selected.scene.clone();
        let mut layers = oversized.layers.to_vec();
        layers[0].spec.canvas = (2, 2);
        oversized.layers = layers.into();
        let mut wide = frame::Video::new(Pixel::YUV420P, 8192, 2);
        wide.data_mut(0).fill(16);
        wide.data_mut(1).fill(128);
        wide.data_mut(2).fill(128);
        let report = cpu.blend_scaled(
            &oversized,
            0,
            Scope::default(),
            &mut wide,
            ColorEncoding::Bt601Limited,
            &mut cache,
        );
        assert_eq!((report.blended, report.unsupported), (0, 1));
        assert!(wide.data(0).iter().all(|pixel| *pixel == 16));
    }

    #[test]
    fn clip_schedule_seeks_render_the_same_cached_pixels_and_opacity() {
        use crate::playlist::{Action, ClipSchedule, Event, Limits};

        let mut cpu = CpuScene::new(1, 128 * 1024);
        let resource = cpu
            .prepare_bitmap(
                ResourceKey { id: 1, revision: 1 },
                &bitmap([255, 40, 0, 180]),
            )
            .unwrap();
        let schedule = ClipSchedule::compile(
            1000,
            &[
                Event {
                    at_us: 100,
                    action: Action::Take {
                        layer: LayerSpec {
                            id: LayerId(1),
                            resource: resource.key,
                            z: 0,
                            scope: Scope::default(),
                            canvas: (8, 8),
                            position: (2, 2),
                            opacity: 0.6,
                        },
                        transition: Transition {
                            fade_in_us: 100,
                            fade_out_us: 100,
                            ..Default::default()
                        },
                    },
                },
                Event {
                    at_us: 500,
                    action: Action::Stop(LayerId(1)),
                },
            ],
            std::slice::from_ref(&resource),
            Limits::default(),
        )
        .unwrap();
        let pixels = cpu.surfaces.get(&resource).unwrap().clone();
        for (time, opacity) in [(150, 0.3), (300, 0.6), (550, 0.3), (800, 0.0), (150, 0.3)] {
            let selected = schedule.frame_at(Some(time)).unwrap();
            let mut actual = background();
            let mut expected = background();
            cpu.blend(
                selected.scene,
                selected.position_us,
                Scope::default(),
                &mut actual,
                ColorEncoding::Bt601Limited,
            );
            let mut overlay = pixels.at(0).unwrap().as_ref().as_ref();
            overlay.x = 2;
            overlay.y = 2;
            blend_overlay(&mut expected, overlay, opacity);
            for plane in 0..3 {
                assert_eq!(actual.data(plane), expected.data(plane));
            }
            assert!(Arc::ptr_eq(&pixels, cpu.surfaces.get(&resource).unwrap()));
        }
    }

    #[test]
    fn layered_static_graphics_match_existing_blending_and_reuse_cached_pixels_during_fades() {
        let mut cpu = CpuScene::new(3, 128 * 1024);
        let (control, mut controller) = SceneController::new(8, 3, 3);
        let mut resources = Vec::new();
        for (index, rgba) in [[255, 0, 0, 255], [255, 255, 255, 128], [0, 0, 255, 200]]
            .into_iter()
            .enumerate()
        {
            let key = ResourceKey {
                id: index as u64,
                revision: index as u64 + 1,
            };
            let resource = cpu.prepare_bitmap(key, &bitmap(rgba)).unwrap();
            control.submit(Command::Prepare(key)).unwrap();
            control.submit(Command::Ready(resource.clone())).unwrap();
            control
                .submit(Command::Take {
                    layer: LayerSpec {
                        id: LayerId(index as u64),
                        resource: key,
                        z: index as i32,
                        scope: Scope::default(),
                        canvas: (8, 8),
                        position: (2, 2),
                        opacity: 0.75,
                    },
                    transition: Transition {
                        fade_in_us: 100,
                        fade_out_us: 100,
                        ..Default::default()
                    },
                })
                .unwrap();
            controller.advance(0);
            resources.push(resource);
        }
        let scene = controller.advance(50);
        let original = cpu.surfaces.get(&resources[0]).unwrap().clone();
        for time in [25, 50, 100, 1_000_000] {
            let mut actual = background();
            let mut expected = background();
            let report = cpu.blend(
                &scene,
                time,
                Scope::default(),
                &mut actual,
                ColorEncoding::Bt601Limited,
            );
            assert_eq!(report.blended, 3);
            for resource in &resources {
                let mut overlay = cpu
                    .surfaces
                    .get(resource)
                    .unwrap()
                    .at(0)
                    .unwrap()
                    .as_ref()
                    .as_ref();
                overlay.x = 2;
                overlay.y = 2;
                blend_overlay(
                    &mut expected,
                    overlay,
                    (time as f64 / 100.0).min(1.0) * 0.75,
                );
            }
            for plane in 0..3 {
                assert_eq!(actual.data(plane), expected.data(plane));
            }
            assert!(Arc::ptr_eq(
                &original,
                cpu.surfaces.get(&resources[0]).unwrap()
            ));
        }
        control.submit(Command::ClearAll).unwrap();
        controller.advance(200);
        drop(resources);
        cpu.retire();
        assert!(
            cpu.surfaces.get(&scene.layers[0].resource).is_some(),
            "queued snapshots must retain ready resources"
        );
    }

    #[test]
    fn unsupported_color_and_canvas_are_skipped_and_invalid_preparation_is_rejected() {
        let mut cpu = CpuScene::new(1, 1);
        assert!(
            cpu.prepare_bitmap(ResourceKey { id: 1, revision: 1 }, &bitmap([255; 4]))
                .is_err()
        );
        let mut cpu = CpuScene::new(1, 128 * 1024);
        let mut invalid = bitmap([255; 4]);
        invalid.pixels.pop();
        assert!(
            cpu.prepare_bitmap(ResourceKey { id: 1, revision: 1 }, &invalid)
                .is_err()
        );
        let resource = cpu
            .prepare_bitmap(ResourceKey { id: 1, revision: 1 }, &bitmap([255; 4]))
            .unwrap();
        let (control, mut controller) = SceneController::new(4, 1, 1);
        control.submit(Command::Prepare(resource.key)).unwrap();
        control.submit(Command::Ready(resource.clone())).unwrap();
        control
            .submit(Command::Take {
                layer: LayerSpec {
                    id: LayerId(1),
                    resource: resource.key,
                    z: 0,
                    scope: Scope::default(),
                    canvas: (8, 8),
                    position: (0, 0),
                    opacity: 1.0,
                },
                transition: Transition::default(),
            })
            .unwrap();
        let scene = controller.advance(0);
        let mut target = background();
        let before = target.data(0).to_vec();
        assert_eq!(
            cpu.blend(
                &scene,
                0,
                Scope::default(),
                &mut target,
                ColorEncoding::Bt709Limited
            )
            .unsupported,
            1
        );
        assert_eq!(target.data(0), before);
        // Strict admission also checks the actual target tags, rather than
        // trusting the caller's compatibility label for an untagged frame.
        let cpu = cpu.with_color_policy(ColorPolicy::MatchingSignal);
        assert_eq!(
            cpu.blend(
                &scene,
                0,
                Scope::default(),
                &mut target,
                ColorEncoding::Bt601Limited
            )
            .unsupported,
            1
        );
        assert_eq!(target.data(0), before);
        let empty = CpuScene::new(1, 1);
        assert_eq!(
            empty
                .blend(
                    &scene,
                    0,
                    Scope::default(),
                    &mut target,
                    ColorEncoding::Bt601Limited
                )
                .missing,
            1
        );
    }
}
