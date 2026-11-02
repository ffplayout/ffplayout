//! Prepared clip graphics. Asset loading stays outside the playback thread.
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock, PoisonError, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use ff_graphics::{
    ffmpeg::scene::{CompositionReport, CpuScene, SceneScaleCache},
    playlist::ClipSchedule,
    scene::{SceneRevision, Scope},
    surface::ColorEncoding,
};
use ffmpeg_next::frame;

type ResolutionCaches = BTreeMap<(u32, u32), Arc<Mutex<SceneScaleCache>>>;

struct Prepared {
    renderer: CpuScene,
    schedule: ClipSchedule,
    reported_failure: AtomicBool,
    caches: Mutex<ResolutionCaches>,
}

/// Prepared immutable assets and schedule for one clip occurrence. Construct on
/// a preparation worker, then move into `ClipPlaybackOptions`. No media I/O is
/// performed during rendering. Dropping a clip leaves only queued frame leases.
#[derive(Clone)]
pub struct ClipGraphics {
    prepared: Arc<OnceLock<Arc<Prepared>>>,
    position_us: i64,
}

impl ClipGraphics {
    pub fn new(renderer: CpuScene, schedule: ClipSchedule) -> Self {
        Self {
            prepared: Arc::new(OnceLock::from(Arc::new(Prepared {
                renderer,
                schedule,
                reported_failure: AtomicBool::new(false),
                caches: Mutex::new(BTreeMap::new()),
            }))),
            position_us: 0,
        }
    }

    /// Publish preparation asynchronously without a channel or a video-thread wait.
    /// Late results use the current clip position; they never restart event fades.
    pub fn deferred() -> (Self, ClipGraphicsPreparation) {
        let prepared = Arc::new(OnceLock::new());
        let completion = ClipGraphicsPreparation {
            destination: Arc::downgrade(&prepared),
        };

        (
            Self {
                prepared,
                position_us: 0,
            },
            completion,
        )
    }

    /// Position within the scheduled item when joining part-way through it.
    /// This is independent of the media file's `in` cut / decoder seek position.
    pub fn at_position(mut self, position_us: i64) -> Self {
        self.position_us = position_us;

        self
    }

    pub(crate) fn frame_at(&self, elapsed_us: i64) -> Option<ClipSceneFrame> {
        let prepared = self.prepared.get()?;
        let selected = prepared
            .schedule
            .frame_at(Some(self.position_us.checked_add(elapsed_us)?))?;

        Some(ClipSceneFrame {
            prepared: prepared.clone(),
            scene: selected.scene.clone(),
            position_us: selected.position_us,
        })
    }
}

/// Weak completion ticket: preparation does not extend a cancelled clip's lifetime.
pub struct ClipGraphicsPreparation {
    destination: Weak<OnceLock<Arc<Prepared>>>,
}

impl ClipGraphicsPreparation {
    pub fn is_cancelled(&self) -> bool {
        self.destination.strong_count() == 0
    }

    /// Returns false when cancelled, already completed, or given an unready handle.
    pub fn publish(self, graphics: ClipGraphics) -> bool {
        let (Some(destination), Some(prepared)) =
            (self.destination.upgrade(), graphics.prepared.get())
        else {
            return false;
        };

        destination.set(prepared.clone()).is_ok()
    }
}

/// Immutable per-video-frame metadata; safe across audio lookahead and fan-out.
#[derive(Clone)]
pub(crate) struct ClipSceneFrame {
    prepared: Arc<Prepared>,
    scene: SceneRevision,
    position_us: i64,
}

impl ClipSceneFrame {
    pub(crate) fn visible(&self) -> bool {
        self.scene
            .evaluate(self.position_us, Scope::default())
            .next()
            .is_some()
    }

    pub(crate) fn blend(&self, target: &mut frame::Video) {
        let resolution = (target.width(), target.height());
        let report = if self
            .scene
            .layers
            .iter()
            .all(|layer| layer.spec.canvas == resolution)
        {
            self.prepared.renderer.blend(
                &self.scene,
                self.position_us,
                Scope::default(),
                target,
                ColorEncoding::Bt601Limited,
            )
        } else {
            self.blend_scaled(target, resolution)
        };
        if (report.missing > 0 || report.unsupported > 0)
            && !self.prepared.reported_failure.swap(true, Ordering::Relaxed)
        {
            log::warn!(
                "Clip graphics skipped unsupported or missing surfaces (missing={}, unsupported={})",
                report.missing,
                report.unsupported
            );
        }
    }

    fn blend_scaled(&self, target: &mut frame::Video, resolution: (u32, u32)) -> CompositionReport {
        let cache = {
            let mut caches = self
                .prepared
                .caches
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            // A misconfigured/dynamically changing destination must not grow a
            // long clip's scaled surface caches without limit.
            if !caches.contains_key(&resolution) && caches.len() >= 8 {
                return CompositionReport {
                    unsupported: 1,
                    ..Default::default()
                };
            }

            caches.entry(resolution).or_default().clone()
        };
        let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);

        self.prepared.renderer.blend_scaled(
            &self.scene,
            self.position_us,
            Scope::default(),
            target,
            ColorEncoding::Bt601Limited,
            &mut cache,
        )
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use ff_graphics::{
        bitmap::StraightRgbaBitmap,
        playback::Transition,
        playlist::{Action, Event, Limits},
        scene::{LayerId, LayerSpec},
        surface::ResourceKey,
    };

    use super::*;

    pub(crate) fn fixture(canvas: (u32, u32)) -> ClipGraphics {
        let mut renderer = CpuScene::new(1, 64 * 1024);
        let key = ResourceKey { id: 1, revision: 1 };
        let resource = renderer
            .prepare_bitmap(
                key,
                &StraightRgbaBitmap {
                    width: 8,
                    height: 4,
                    pixels: [255, 255, 255, 180].repeat(32),
                },
            )
            .unwrap();
        let schedule = ClipSchedule::compile(
            400_000,
            &[
                Event {
                    at_us: 0,
                    action: Action::Take {
                        layer: LayerSpec {
                            id: LayerId(1),
                            resource: key,
                            z: 0,
                            scope: Scope::default(),
                            canvas,
                            position: (2, 2),
                            opacity: 0.5,
                        },
                        transition: Transition {
                            fade_in_us: 80_000,
                            fade_out_us: 80_000,
                            ..Default::default()
                        },
                    },
                },
                Event {
                    at_us: 240_000,
                    action: Action::Stop(LayerId(1)),
                },
            ],
            &[resource],
            Limits::default(),
        )
        .unwrap();

        ClipGraphics::new(renderer, schedule)
    }

    #[test]
    fn queued_frame_owns_assets_after_clip_and_playback_request_are_dropped() {
        let graphics = fixture((32, 16));
        let weak = Arc::downgrade(graphics.prepared.get().unwrap());
        let queued = graphics.frame_at(120_000).unwrap();
        let mut target = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 16, 8);
        queued.blend(&mut target);
        assert_eq!(
            graphics
                .prepared
                .get()
                .unwrap()
                .caches
                .lock()
                .unwrap()
                .len(),
            1
        );
        drop(graphics);
        assert!(weak.upgrade().is_some());
        assert!(queued.visible());
        drop(queued);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn joining_inside_clip_preserves_phase_and_never_restarts_after_end() {
        let graphics = fixture((32, 16)).at_position(40_000);
        assert_eq!(graphics.frame_at(0).unwrap().position_us, 40_000);
        assert!(graphics.frame_at(360_000).is_none());
        assert!(graphics.frame_at(i64::MAX).is_none());
    }

    #[test]
    fn late_preparation_uses_current_position_and_cancelled_results_are_discarded() {
        let (pending, completion) = ClipGraphics::deferred();
        let pending = pending.at_position(40_000);
        assert!(pending.frame_at(0).is_none());
        assert!(completion.publish(fixture((32, 16))));
        let selected = pending.frame_at(80_000).unwrap();
        assert_eq!(selected.position_us, 120_000);
        assert_eq!(
            selected
                .scene
                .evaluate(selected.position_us, Scope::default())
                .next()
                .unwrap()
                .opacity,
            0.5
        );
        assert!(!pending.frame_at(320_000).unwrap().visible());
        let (cancelled, completion) = ClipGraphics::deferred();
        drop(cancelled);
        assert!(completion.is_cancelled());
        assert!(!completion.publish(fixture((32, 16))));
    }

    #[test]
    fn unavailable_surfaces_leave_video_intact_and_report_only_once() {
        let mut graphics = fixture((32, 16));
        let prepared = Arc::get_mut(&mut graphics.prepared)
            .unwrap()
            .get_mut()
            .unwrap();
        Arc::get_mut(prepared).unwrap().renderer = CpuScene::new(0, 0);
        let selected = graphics.frame_at(120_000).unwrap();
        let mut target = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 32, 16);
        for plane in 0..3 {
            target.data_mut(plane).fill(128);
        }
        for _ in 0..3 {
            selected.blend(&mut target);
            assert!(target.data(0).iter().all(|byte| *byte == 128));
        }
        assert!(
            graphics
                .prepared
                .get()
                .unwrap()
                .reported_failure
                .load(Ordering::Relaxed)
        );
    }
}
