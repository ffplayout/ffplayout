//! Prepared clip graphics. Asset loading stays outside the playback thread.
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock, PoisonError, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use ff_graphics::{
    ffmpeg::scene::{CompositionReport, CpuScene, FrozenScene, SceneScaleCache},
    playlist::ClipSchedule,
    scene::Scope,
    surface::ColorEncoding,
};
use ffmpeg_next::frame;

type ResolutionCaches = BTreeMap<(u32, u32), Arc<Mutex<SceneScaleCache>>>;

struct Prepared {
    renderer: CpuScene,
    schedule: ClipSchedule,
    reported_failure: AtomicBool,
    caches: Mutex<ResolutionCaches>,
    snapshots: Mutex<Vec<Arc<FrozenScene>>>,
    owners: AtomicUsize,
}

impl Prepared {
    fn snapshot(&self, frozen: FrozenScene) -> Arc<FrozenScene> {
        // Recycle metadata storage after queued consumers release it. Contention
        // must never make the program thread wait for another output.
        if let Ok(mut snapshots) = self.snapshots.try_lock() {
            // A temporary queue peak must not leave old video-sized graphics
            // pinned in metadata slots that are no longer needed.
            for snapshot in snapshots.iter_mut() {
                if let Some(slot) = Arc::get_mut(snapshot) {
                    slot.clear();
                }
            }
            for snapshot in snapshots.iter_mut() {
                if let Some(slot) = Arc::get_mut(snapshot) {
                    *slot = frozen;
                    return snapshot.clone();
                }
            }
            let snapshot = Arc::new(frozen);
            if snapshots.len() < 64 {
                snapshots.push(snapshot.clone());
            }

            return snapshot;
        }

        Arc::new(frozen)
    }
}

/// Playback handles own slots; queued frames own only ready scene pixels/state.
/// Multiple slots may share prepared data after asynchronous publication.
struct PreparedSlot(OnceLock<Arc<Prepared>>);

impl PreparedSlot {
    fn get(&self) -> Option<&Arc<Prepared>> {
        self.0.get()
    }

    fn publish(&self, prepared: Arc<Prepared>) -> bool {
        prepared.owners.fetch_add(1, Ordering::AcqRel);
        match self.0.set(prepared) {
            Ok(()) => true,
            Err(prepared) => {
                prepared.owners.fetch_sub(1, Ordering::AcqRel);
                false
            }
        }
    }
}

impl Drop for PreparedSlot {
    fn drop(&mut self) {
        if let Some(prepared) = self.0.get()
            && prepared.owners.fetch_sub(1, Ordering::AcqRel) == 1
        {
            prepared.renderer.stop_streams();
        }
    }
}

/// Prepared immutable assets and schedule for one clip occurrence. Construct on
/// a preparation worker, then move into `ClipPlaybackOptions`. No media I/O is
/// performed during rendering. Dropping a clip leaves only queued frame leases.
#[derive(Clone)]
pub struct ClipGraphics {
    prepared: Arc<PreparedSlot>,
    position_us: i64,
}

impl ClipGraphics {
    pub fn new(renderer: CpuScene, schedule: ClipSchedule) -> Self {
        let prepared = Arc::new(PreparedSlot(OnceLock::new()));
        prepared.publish(Arc::new(Prepared {
            renderer,
            schedule,
            reported_failure: AtomicBool::new(false),
            caches: Mutex::new(BTreeMap::new()),
            snapshots: Mutex::new(Vec::new()),
            owners: AtomicUsize::new(0),
        }));

        Self {
            prepared,
            position_us: 0,
        }
    }

    /// Publish preparation asynchronously without a channel or a video-thread wait.
    /// Late results use the current clip position; they never restart event fades.
    pub fn deferred() -> (Self, ClipGraphicsPreparation) {
        let prepared = Arc::new(PreparedSlot(OnceLock::new()));
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

    pub(crate) fn suspend(&self) {
        if let Some(prepared) = self.prepared.get() {
            prepared.renderer.suspend_streams();
        }
    }

    pub(crate) fn frame_at(&self, elapsed_us: i64) -> Option<ClipSceneFrame> {
        let prepared = self.prepared.get()?;
        let position_us = self.position_us.checked_add(elapsed_us)?;
        let frozen =
            prepared
                .renderer
                .freeze_schedule(&prepared.schedule, position_us, Scope::default())?;
        let selected = prepared.schedule.frame_at(Some(position_us))?;
        #[cfg(not(test))]
        let _ = selected;

        Some(ClipSceneFrame {
            #[cfg(test)]
            scene: selected.scene.clone(),
            #[cfg(test)]
            position_us: selected.position_us,
            prepared: prepared.clone(),
            frozen: prepared.snapshot(frozen),
        })
    }
}

/// Weak completion ticket: preparation does not extend a cancelled clip's lifetime.
pub struct ClipGraphicsPreparation {
    destination: Weak<PreparedSlot>,
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

        destination.publish(prepared.clone())
    }
}

/// Immutable per-video-frame metadata; safe across audio lookahead and fan-out.
#[derive(Clone)]
pub(crate) struct ClipSceneFrame {
    prepared: Arc<Prepared>,
    frozen: Arc<FrozenScene>,
    #[cfg(test)]
    scene: ff_graphics::scene::SceneRevision,
    #[cfg(test)]
    position_us: i64,
}

impl ClipSceneFrame {
    pub(crate) fn visible(&self) -> bool {
        !self.frozen.is_empty()
    }

    pub(crate) fn blend(&self, target: &mut frame::Video) {
        let resolution = (target.width(), target.height());
        let report = if self.frozen.matches_canvas(resolution) {
            self.frozen.blend(target, ColorEncoding::Bt601Limited)
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

        self.frozen
            .blend_scaled(target, ColorEncoding::Bt601Limited, &mut cache)
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
    fn publishing_a_stream_keeps_its_worker_until_playback_ends_but_queued_pixels_survive() {
        use ff_graphics::{
            ffmpeg::sources::animation::AnimationOptions, playback::Repeat,
            preparation::PreparationService, source::Completion,
        };
        use std::{
            path::Path,
            time::{Duration, Instant},
        };
        let service = PreparationService::default();
        let mut renderer = CpuScene::new(1, 64 * 1024 * 1024).with_preparation(&service);
        let resource = renderer
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests_assets/storage/media_images/animated-logo-video.mkv"),
                AnimationOptions {
                    size: Some((16, 16)),
                    repeat: Repeat::Forever,
                    completion: Completion::Hide,
                },
                || false,
            )
            .unwrap();
        let schedule = ClipSchedule::compile(
            20_000_000,
            &[Event {
                at_us: 0,
                action: Action::Take {
                    layer: LayerSpec {
                        id: LayerId(1),
                        resource: resource.key,
                        z: 0,
                        scope: Scope::default(),
                        canvas: (32, 32),
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
        let (pending, completion) = ClipGraphics::deferred();
        let graphics = ClipGraphics::new(renderer, schedule);
        assert!(completion.publish(graphics.clone()));
        drop(graphics);
        let deadline = Instant::now() + Duration::from_secs(5);
        let queued = loop {
            let queued = pending.frame_at(6_100_000).unwrap();
            if queued.visible() {
                break queued;
            }
            assert!(
                Instant::now() < deadline,
                "publication must not cancel the decoder"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        drop(pending);
        assert!(
            service.join(Duration::from_secs(2)),
            "clip end must stop its worker even with queued frames"
        );
        assert_eq!(service.budget().usage().2, 0);
        let mut target = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 32, 32);
        for plane in 0..3 {
            target.data_mut(plane).fill(128);
        }
        queued.blend(&mut target);
        assert!(service.budget().usage().0 > 0);
        drop(queued);
        assert_eq!(service.budget().usage().0, 0);
    }

    #[test]
    fn a_scheduled_stream_prepares_before_the_first_take_without_publishing_graphics() {
        use ff_graphics::{
            ffmpeg::sources::animation::AnimationOptions, playback::Repeat,
            preparation::PreparationService, source::Completion,
        };
        use std::{
            path::Path,
            time::{Duration, Instant},
        };
        let service = PreparationService::default();
        let mut renderer = CpuScene::new(1, 64 * 1024 * 1024).with_preparation(&service);
        let resource = renderer
            .prepare_animation(
                ResourceKey { id: 1, revision: 1 },
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests_assets/storage/media_images/animated-logo-video.mkv"),
                AnimationOptions {
                    size: Some((16, 16)),
                    repeat: Repeat::Forever,
                    completion: Completion::Hide,
                },
                || false,
            )
            .unwrap();
        let poster_bytes = resource.descriptor.storage_bytes;
        let schedule = ClipSchedule::compile(
            5_000_000,
            &[Event {
                at_us: 2_000_000,
                action: Action::Take {
                    layer: LayerSpec {
                        id: LayerId(1),
                        resource: resource.key,
                        z: 0,
                        scope: Scope::default(),
                        canvas: (32, 32),
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
        let graphics = ClipGraphics::new(renderer, schedule);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                graphics.frame_at(1_600_000).is_none(),
                "preparation must not publish future graphics"
            );
            if service.budget().usage().0 > poster_bytes {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "no decoder work before first take"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let queued = graphics.frame_at(2_000_000).unwrap();
        assert!(queued.visible());
        drop(queued);
        graphics.suspend();
        drop(graphics);
        assert!(service.join(Duration::from_secs(2)));
        assert_eq!(service.budget().usage(), (0, 0, 0));
        assert!(service.shutdown(Duration::from_secs(2)));
    }

    #[test]
    fn recycled_snapshot_slots_release_pixels_after_a_temporary_queue_peak() {
        let graphics = fixture((32, 16));
        let queued = [40_000, 80_000, 120_000].map(|time| graphics.frame_at(time).unwrap());
        assert_eq!(
            graphics
                .prepared
                .get()
                .unwrap()
                .snapshots
                .lock()
                .unwrap()
                .len(),
            3
        );
        drop(queued);
        let _current = graphics.frame_at(160_000).unwrap();
        let pool = graphics.prepared.get().unwrap().snapshots.lock().unwrap();
        assert_eq!(
            pool.iter().filter(|snapshot| !snapshot.is_empty()).count(),
            1
        );
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
                .content
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
            .0
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
