//! Channel-owned manual program graphics. Preparation is supplied by the application.
use std::{
    array, fmt,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicI64, Ordering},
    },
};

use anyhow::{Result, anyhow, ensure};
use ff_graphics::scene::SceneRevision;
use ffmpeg_next::frame;

use super::clip::{ClipGraphics, ClipSceneFrame};

const MAX_LAYERS: usize = 8;

#[derive(Clone)]
struct Layer {
    id: u64,
    origin: i64,
    graphics: ClipGraphics,
    scene: SceneRevision,
    rendered: bool,
}

#[derive(Default)]
struct State {
    revision: u64,
    layers: Vec<Layer>,
    snapshots: Vec<Arc<ManualFrame>>,
}

#[derive(Clone, Default)]
pub struct ManualGraphics {
    state: Arc<Mutex<State>>,
    clock: Arc<AtomicI64>,
}

impl fmt::Debug for ManualGraphics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManualGraphics").finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct ManualLayerStatus {
    pub id: u64,
    pub visible: bool,
    pub active: bool,
    pub position: (i32, i32),
    pub opacity: f64,
    pub z: i32,
}

pub struct ManualChange<'a> {
    pub id: Option<u64>,
    pub operation: &'a str,
    pub position: Option<(i32, i32)>,
    pub opacity: Option<f64>,
    pub z: Option<i32>,
    pub visible: Option<bool>,
}

pub(crate) struct ManualFrame {
    frames: [Option<ClipSceneFrame>; MAX_LAYERS],
}

impl ManualFrame {
    pub(crate) fn blend(&self, target: &mut frame::Video) {
        for frame in self.frames.iter().flatten() {
            frame.blend(target);
        }
    }
}

impl ManualGraphics {
    pub fn revision(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .revision
    }

    pub fn status(&self) -> (u64, Vec<ManualLayerStatus>) {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let layers = state
            .layers
            .iter()
            .filter_map(|slot| {
                let layer = slot.scene.layers.first()?;
                Some(ManualLayerStatus {
                    id: slot.id,
                    visible: layer.visible && layer.presentation.is_visible(),
                    active: slot.rendered && layer.visible,
                    position: layer.spec.position,
                    opacity: layer.spec.opacity,
                    z: layer.spec.z,
                })
            })
            .collect();

        (state.revision, layers)
    }

    pub fn take(&self, expected: u64, id: u64, graphics: ClipGraphics) -> Result<u64> {
        let scene = graphics
            .initial_scene()
            .ok_or_else(|| anyhow!("graphic is not ready"))?;
        ensure!(
            scene.layers.len() == 1,
            "a manual take requires one ready layer"
        );
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        ensure!(
            state.revision == expected,
            "graphics revision changed; refresh before retrying"
        );
        ensure!(
            state.layers.len() < MAX_LAYERS || state.layers.iter().any(|slot| slot.id == id),
            "manual layer limit exceeded (8)"
        );
        state.layers.retain(|slot| slot.id != id);
        state.layers.push(Layer {
            id,
            origin: self.clock.load(Ordering::Acquire),
            graphics,
            scene,
            rendered: false,
        });
        state
            .layers
            .sort_by_key(|slot| (slot.scene.layers[0].spec.z, slot.id));
        state.snapshots.retain(|frame| Arc::strong_count(frame) > 1);
        state.revision += 1;

        Ok(state.revision)
    }

    pub fn change(&self, expected: u64, change: ManualChange<'_>) -> Result<u64> {
        let ManualChange {
            id,
            operation,
            position,
            opacity,
            z,
            visible,
        } = change;
        ensure!(
            matches!(
                operation,
                "update" | "stop" | "clear" | "clear_all" | "visibility"
            ),
            "unknown graphics operation"
        );
        ensure!(
            opacity.is_none_or(|value| value.is_finite() && (0.0..=1.0).contains(&value)),
            "invalid opacity"
        );
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        ensure!(
            state.revision == expected,
            "graphics revision changed; refresh before retrying"
        );

        if operation == "clear_all" {
            state.layers.clear();
        } else {
            let slot = state
                .layers
                .iter_mut()
                .find(|slot| Some(slot.id) == id)
                .ok_or_else(|| anyhow!("unknown manual layer"))?;

            if operation == "clear" {
                state.layers.retain(|slot| Some(slot.id) != id);
            } else {
                let mut layers = slot.scene.layers.to_vec();
                let layer = &mut layers[0];

                match operation {
                    "stop" => layer.presentation.stop(
                        self.clock
                            .load(Ordering::Acquire)
                            .saturating_sub(slot.origin),
                    ),
                    "visibility" => layer.presentation.set_visible(
                        visible.unwrap_or(true),
                        self.clock
                            .load(Ordering::Acquire)
                            .saturating_sub(slot.origin),
                    ),
                    _ => {
                        if let Some(value) = position {
                            layer.spec.position = value;
                        }

                        if let Some(value) = opacity {
                            layer.spec.opacity = value;
                        }

                        if let Some(value) = z {
                            layer.spec.z = value;
                        }
                    }
                }

                slot.scene.layers = layers.into();
            }
        }

        state
            .layers
            .sort_by_key(|slot| (slot.scene.layers[0].spec.z, slot.id));
        state.snapshots.retain(|frame| Arc::strong_count(frame) > 1);
        state.revision += 1;

        Ok(state.revision)
    }

    pub fn clear(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.layers.clear();
        state.snapshots.clear();
        state.revision += 1;
    }

    pub(crate) fn frame_at(&self, now: i64) -> Result<Option<Arc<ManualFrame>>, ()> {
        self.clock.store(now, Ordering::Release);
        let mut state = self.state.try_lock().map_err(|_| ())?;

        if state.layers.is_empty() {
            state.snapshots.clear();

            return Ok(None);
        }

        for snapshot in &mut state.snapshots {
            if let Some(slot) = Arc::get_mut(snapshot) {
                slot.frames = array::from_fn(|_| None);
            }
        }

        let frames = array::from_fn(|index| {
            let slot = state.layers.get_mut(index)?;
            let frame = slot
                .graphics
                .frame_scene(&slot.scene, now.saturating_sub(slot.origin))
                .filter(ClipSceneFrame::visible);
            slot.rendered = frame.is_some();

            frame
        });

        if frames.iter().all(Option::is_none) {
            return Ok(None);
        }

        for snapshot in &mut state.snapshots {
            if let Some(slot) = Arc::get_mut(snapshot) {
                slot.frames = frames;

                return Ok(Some(snapshot.clone()));
            }
        }

        let snapshot = Arc::new(ManualFrame { frames });

        if state.snapshots.len() < 32 {
            state.snapshots.push(snapshot.clone());
        }

        Ok(Some(snapshot))
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{format::Pixel, frame};

    use crate::graphics::clip::tests::fixture;

    use super::*;

    fn operation(id: u64, operation: &str) -> ManualChange<'_> {
        ManualChange {
            id: Some(id),
            operation,
            position: None,
            opacity: None,
            z: None,
            visible: None,
        }
    }

    #[test]
    fn manual_layer_admission_is_bounded_and_replacement_does_not_need_a_spare_slot() {
        let graphics = ManualGraphics::default();

        for id in 0..MAX_LAYERS as u64 {
            graphics.take(id, id, fixture((32, 16))).unwrap();
        }

        assert!(
            graphics
                .take(MAX_LAYERS as u64, 99, fixture((32, 16)))
                .is_err()
        );
        graphics
            .take(MAX_LAYERS as u64, 0, fixture((32, 16)))
            .unwrap();
        assert_eq!(graphics.status().1.len(), MAX_LAYERS);
        graphics.clear();
        assert!(graphics.status().1.is_empty());
    }

    #[test]
    fn manual_controls_preserve_queued_pixels_and_reject_stale_revisions() {
        let graphics = ManualGraphics::default();
        let other = ManualGraphics::default();
        graphics.frame_at(1_000_000).unwrap();
        graphics.take(0, 7, fixture((32, 16))).unwrap();
        let queued = graphics.frame_at(1_100_000).unwrap().unwrap();
        assert_eq!(graphics.status().1[0].id, 7);
        assert!(graphics.take(0, 8, fixture((32, 16))).is_err());
        assert!(other.status().1.is_empty());
        assert!(graphics.change(0, operation(7, "clear")).is_err());
        graphics
            .change(
                1,
                ManualChange {
                    visible: Some(false),
                    ..operation(7, "visibility")
                },
            )
            .unwrap();
        assert!(graphics.frame_at(1_110_000).unwrap().is_some());
        assert!(graphics.frame_at(1_180_000).unwrap().is_none());
        graphics
            .change(
                2,
                ManualChange {
                    visible: Some(true),
                    ..operation(7, "visibility")
                },
            )
            .unwrap();
        assert!(graphics.frame_at(1_220_000).unwrap().is_some());
        graphics.change(3, operation(7, "stop")).unwrap();
        assert!(graphics.frame_at(1_300_000).unwrap().is_none());
        graphics.change(4, operation(7, "clear")).unwrap();
        assert!(graphics.status().1.is_empty());
        let mut pixels = frame::Video::new(Pixel::YUV420P, 32, 16);

        for plane in 0..3 {
            pixels.data_mut(plane).fill(16);
        }

        queued.blend(&mut pixels);
        assert!(pixels.data(0).iter().any(|value| *value > 16));
    }

    #[test]
    fn show_hide_fades_pixels_preserves_queued_frames_and_reports_outgoing_activity() {
        fn luma(snapshot: Option<&ManualFrame>) -> u8 {
            let mut pixels = frame::Video::new(Pixel::YUV420P, 32, 16);

            for plane in 0..3 {
                pixels.data_mut(plane).fill(128);
            }

            if let Some(snapshot) = snapshot {
                snapshot.blend(&mut pixels);
            }

            pixels.data(0)[2 * pixels.stride(0) + 2]
        }

        let graphics = ManualGraphics::default();
        graphics.take(0, 1, fixture((32, 16))).unwrap();
        let queued = graphics.frame_at(100_000).unwrap().unwrap();
        assert_eq!(luma(Some(&queued)), 166);
        graphics
            .change(
                1,
                ManualChange {
                    visible: Some(false),
                    ..operation(1, "visibility")
                },
            )
            .unwrap();
        let outgoing = graphics.frame_at(140_000).unwrap().unwrap();
        assert_eq!(luma(Some(&outgoing)), 147);
        let status = &graphics.status().1[0];
        assert!(!status.visible);
        assert!(status.active);
        assert!(graphics.frame_at(180_000).unwrap().is_none());
        assert!(!graphics.status().1[0].active);
        graphics
            .change(
                2,
                ManualChange {
                    visible: Some(true),
                    ..operation(1, "visibility")
                },
            )
            .unwrap();
        assert!(graphics.frame_at(180_000).unwrap().is_none());
        assert_eq!(luma(graphics.frame_at(220_000).unwrap().as_deref()), 147);
        assert_eq!(luma(graphics.frame_at(260_000).unwrap().as_deref()), 166);
        assert_eq!(luma(Some(&queued)), 166);
        assert_eq!(luma(Some(&outgoing)), 147);
        let state = graphics.state.lock().unwrap();
        assert_eq!(state.layers[0].origin, 0);
        assert_eq!(
            state.layers[0].scene.layers[0]
                .presentation
                .elapsed_us(260_000),
            260_000
        );
    }

    #[test]
    fn manual_graphics_survive_clip_boundaries_during_control_contention() {
        use crate::{
            graphics::{program::ProgramGraphicsOutput, source::ProgramGraphicsState},
            output::FrameOutput,
            utils::config::OutputConfig,
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
                self.0.push(frame.data(0)[2 * frame.stride(0) + 2]);

                Ok(())
            }
        }

        let config = OutputConfig::new(32, 16, 25, 48000);
        config
            .manual_graphics
            .take(0, 1, fixture((32, 16)))
            .unwrap();
        let mut state = ProgramGraphicsState::default();
        let mut capture = Pixels::default();
        let mut source = frame::Video::new(Pixel::YUV420P, 32, 16);

        for plane in 0..3 {
            source.data_mut(plane).fill(128);
        }

        source.set_pts(Some(3));
        ProgramGraphicsOutput::new(&mut capture, &mut state, &config)
            .encode_video(&source)
            .unwrap();
        let guard = config.manual_graphics.state.lock().unwrap();
        source.set_pts(Some(4));
        ProgramGraphicsOutput::new(&mut capture, &mut state, &config)
            .encode_video(&source)
            .unwrap();
        drop(guard);
        config.manual_graphics.clear();
        source.set_pts(Some(5));
        ProgramGraphicsOutput::new(&mut capture, &mut state, &config)
            .encode_video(&source)
            .unwrap();
        assert_eq!(capture.0, [166, 166, 128]);
        assert!(state.manual_frame.is_none());
    }

    #[test]
    fn frame_contention_retains_the_last_snapshot_and_pool_is_reused() {
        let graphics = ManualGraphics::default();
        graphics.take(0, 1, fixture((32, 16))).unwrap();

        for time in 1..100 {
            drop(graphics.frame_at(time * 1000).unwrap());
        }

        assert!(graphics.state.lock().unwrap().snapshots.len() <= 2);
        let guard = graphics.state.lock().unwrap();
        assert!(graphics.frame_at(200_000).is_err());
        drop(guard);
        graphics.clear();
        assert!(graphics.frame_at(201_000).unwrap().is_none());
        assert!(graphics.state.lock().unwrap().snapshots.is_empty());
    }
}
