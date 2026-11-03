//! Immutable clip-local schedules. Compilation is control-side work, not rendering.
use std::{collections::BTreeSet, sync::Arc};

use anyhow::{Context, Result, anyhow, ensure};

use crate::{
    playback::Transition,
    scene::{Command, LayerId, LayerSpec, SceneController, SceneRevision},
    surface::Resource,
};

/// Times are relative to the item's effective playback start, after its `in` cut.
/// Equal timestamps execute in declaration order; the last resulting state wins.
#[derive(Clone, Debug)]
pub struct Event {
    pub at_us: i64,
    pub action: Action,
}

#[derive(Clone, Debug)]
pub enum Action {
    Take {
        layer: LayerSpec,
        transition: Transition,
    },
    Update(LayerSpec),
    Stop(LayerId),
    Visibility {
        layer: LayerId,
        visible: bool,
    },
    Clear(LayerId),
    /// Clears this clip's layers, without releasing prepared future resources.
    ClearAll,
}

/// Bounds both preparation inputs and the retained immutable scene history.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub events: usize,
    pub layers: usize,
    pub resources: usize,
    pub retained_layers: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            events: 256,
            layers: 32,
            resources: 64,
            retained_layers: 4096,
        }
    }
}

struct Boundary {
    at_us: i64,
    scene: SceneRevision,
}

/// One playback occurrence owns one schedule. Skipping/interruption drops it;
/// repeating a playlist item creates another occurrence with its own local clock.
/// This adapter does not own global logo/text layers or perform media I/O.
pub struct ClipSchedule {
    duration_us: i64,
    boundaries: Vec<Boundary>,
}

/// The backend must evaluate the scene at `position_us`, not absolute program PTS.
pub struct ScheduledFrame<'a> {
    pub scene: &'a SceneRevision,
    pub position_us: i64,
}

impl ClipSchedule {
    /// Compile once after asynchronous resource preparation. Resource leases must
    /// come from the backend that will compose these scenes. Each resource ID has
    /// one revision in a schedule; use distinct IDs for distinct prepared assets.
    /// No decoder, renderer,
    /// channel or mutable event cursor is used when selecting subsequent frames.
    pub fn compile(
        duration_us: i64,
        events: &[Event],
        resources: &[Arc<Resource>],
        limits: Limits,
    ) -> Result<Self> {
        ensure!(duration_us > 0, "clip duration must be positive");
        ensure!(
            events.len() <= limits.events,
            "graphics event budget exceeded"
        );
        ensure!(
            resources.len() <= limits.resources,
            "graphics resource budget exceeded"
        );
        let mut previous = 0;
        for event in events {
            ensure!(
                event.at_us >= previous && event.at_us < duration_us,
                "graphics events must be ordered within the effective clip duration"
            );
            previous = event.at_us;
        }

        let (control, mut controller) = SceneController::new(1, limits.layers, limits.resources);
        let apply = |controller: &mut SceneController, command, at_us| {
            let receipt = control
                .submit(command)
                .map_err(|_| anyhow!("graphics compilation queue unavailable"))?;
            let scene = controller.advance(at_us);
            receipt
                .recv()
                .context("graphics compilation acknowledgement missing")?
                .outcome
                .map_err(|reason| anyhow!("invalid graphics event: {reason:?}"))?;

            Ok::<_, anyhow::Error>(scene)
        };
        // The controller uses globally increasing preparation revisions. Sorting
        // here permits any caller resource order without changing event order.
        let mut resources = resources.to_vec();
        resources.sort_by_key(|resource| resource.key.revision);
        let mut resource_ids = BTreeSet::new();
        for resource in resources {
            ensure!(
                resource_ids.insert(resource.key.id),
                "a graphics schedule needs distinct resource IDs"
            );
            apply(&mut controller, Command::Prepare(resource.key), 0)
                .context("preparing graphics schedule resource")?;
            apply(&mut controller, Command::Ready(resource), 0)
                .context("registering graphics schedule resource")?;
        }

        let mut boundaries: Vec<Boundary> = Vec::with_capacity(events.len());
        let mut retained_layers = 0;
        for (index, event) in events.iter().enumerate() {
            let mut scene = controller.advance(event.at_us);
            if matches!(event.action, Action::ClearAll) {
                // Controller ClearAll also cancels resource preparation. A clip
                // clear only affects visible layers, keeping later takes valid.
                for layer in scene.layers.clone().iter() {
                    scene = apply(&mut controller, Command::Clear(layer.spec.id), event.at_us)?;
                }
            } else {
                let command = match event.action {
                    Action::Take { layer, transition } => Command::Take { layer, transition },
                    Action::Update(spec) => Command::Update(spec),
                    Action::Stop(layer) => Command::Stop(layer),
                    Action::Visibility { layer, visible } => Command::Visibility { layer, visible },
                    Action::Clear(layer) => Command::Clear(layer),
                    Action::ClearAll => unreachable!(),
                };
                scene = apply(&mut controller, command, event.at_us)
                    .with_context(|| format!("graphics event {index} at {} us", event.at_us))?;
            }

            let layer_count = scene.layers.len();
            let replaced_layers = if let Some(last) = boundaries.last_mut()
                && last.at_us == event.at_us
            {
                let previous_count = last.scene.layers.len();
                last.scene = scene;

                previous_count
            } else {
                boundaries.push(Boundary {
                    at_us: event.at_us,
                    scene,
                });

                0
            };
            retained_layers = (retained_layers - replaced_layers)
                .checked_add(layer_count)
                .context("graphics scene history size overflow")?;
            ensure!(
                retained_layers <= limits.retained_layers,
                "graphics scene history budget exceeded"
            );
        }

        Ok(Self {
            duration_us,
            boundaries,
        })
    }

    /// `None` suppresses clip graphics during a live takeover. On return, pass
    /// the current scheduled clip position: missed events are not replayed.
    /// Seeking in either direction immediately selects the corresponding state.
    /// An ended/shortened item must never be queried with its old position.
    pub fn frame_at(&self, position_us: Option<i64>) -> Option<ScheduledFrame<'_>> {
        let position_us = position_us?;
        if !(0..self.duration_us).contains(&position_us) {
            return None;
        }
        let index = self
            .boundaries
            .partition_point(|boundary| boundary.at_us <= position_us)
            .checked_sub(1)?;

        Some(ScheduledFrame {
            scene: &self.boundaries[index].scene,
            position_us,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::{
        playback::Repeat,
        scene::Scope,
        surface::{Alpha, ColorEncoding, PixelLayout, ResourceKey, SurfaceDescriptor},
    };

    fn resource() -> Arc<Resource> {
        Arc::new(Resource {
            key: ResourceKey { id: 1, revision: 1 },
            descriptor: SurfaceDescriptor {
                width: 2,
                height: 2,
                layout: PixelLayout::Rgba8,
                alpha: Alpha::Straight,
                color: ColorEncoding::Srgb,
                storage_bytes: 16,
                metadata: None,
            },
        })
    }

    fn layer(id: u64, z: i32) -> LayerSpec {
        LayerSpec {
            id: LayerId(id),
            resource: resource().key,
            z,
            scope: Scope::default(),
            canvas: (1280, 720),
            position: (0, 0),
            opacity: 0.5,
        }
    }

    fn take(at_us: i64, id: u64, z: i32, transition: Transition) -> Event {
        Event {
            at_us,
            action: Action::Take {
                layer: layer(id, z),
                transition,
            },
        }
    }

    fn visible(schedule: &ClipSchedule, at: i64) -> Vec<(u64, f64)> {
        schedule.frame_at(Some(at)).map_or_else(Vec::new, |frame| {
            frame
                .scene
                .evaluate(frame.position_us, Scope::default())
                .map(|layer| (layer.id.0, layer.content.opacity))
                .collect()
        })
    }

    #[test]
    fn seek_pause_and_live_resume_reconstruct_without_restarting_fades() {
        let schedule = ClipSchedule::compile(
            20_000_000,
            &[
                take(
                    2_000_000,
                    1,
                    0,
                    Transition {
                        fade_in_us: 2_000_000,
                        fade_out_us: 2_000_000,
                        ..Default::default()
                    },
                ),
                Event {
                    at_us: 8_000_000,
                    action: Action::Stop(LayerId(1)),
                },
            ],
            &[resource()],
            Limits::default(),
        )
        .unwrap();
        assert!(visible(&schedule, 1_000_000).is_empty());
        assert_eq!(visible(&schedule, 3_000_000), [(1, 0.25)]);
        assert_eq!(visible(&schedule, 9_000_000), [(1, 0.25)]);
        assert!(schedule.frame_at(None).is_none());
        assert!(visible(&schedule, 11_000_000).is_empty());
        assert_eq!(visible(&schedule, 3_000_000), [(1, 0.25)]);
        let first = schedule.frame_at(Some(3_000_000)).unwrap();
        let paused = schedule.frame_at(Some(3_000_000)).unwrap();
        assert!(std::ptr::eq(first.scene, paused.scene));
        assert!(schedule.frame_at(Some(-1)).is_none());
        assert!(schedule.frame_at(Some(20_000_000)).is_none());
    }

    #[test]
    fn same_timestamp_order_updates_z_and_opacity_without_restarting_transition() {
        let mut updated = layer(1, 20);
        updated.opacity = 0.8;
        let schedule = ClipSchedule::compile(
            100,
            &[
                take(
                    0,
                    1,
                    0,
                    Transition {
                        fade_in_us: 20,
                        ..Default::default()
                    },
                ),
                take(0, 2, 10, Transition::default()),
                Event {
                    at_us: 10,
                    action: Action::Update(updated),
                },
            ],
            &[resource()],
            Limits::default(),
        )
        .unwrap();
        assert_eq!(visible(&schedule, 10), [(2, 0.5), (1, 0.4)]);
        assert_eq!(visible(&schedule, 20), [(2, 0.5), (1, 0.8)]);
    }

    #[test]
    fn clear_preserves_preparation_and_repeated_takes_have_their_own_phase() {
        let schedule = ClipSchedule::compile(
            100,
            &[
                take(0, 1, 0, Transition::default()),
                Event {
                    at_us: 5,
                    action: Action::ClearAll,
                },
                take(
                    5,
                    1,
                    0,
                    Transition {
                        fade_in_us: 10,
                        hold_us: Some(10),
                        repeat: Repeat::Count(NonZeroU32::new(2).unwrap()),
                        ..Default::default()
                    },
                ),
            ],
            &[resource()],
            Limits::default(),
        )
        .unwrap();
        assert_eq!(visible(&schedule, 10), [(1, 0.25)]);
        assert_eq!(visible(&schedule, 30), [(1, 0.25)]);
        assert!(visible(&schedule, 45).is_empty());
        assert_eq!(visible(&schedule, 0), [(1, 0.5)]);
    }

    #[test]
    fn expired_layer_id_can_be_reused_and_forever_ends_with_clip() {
        let schedule = ClipSchedule::compile(
            100,
            &[
                take(
                    0,
                    1,
                    0,
                    Transition {
                        hold_us: Some(10),
                        ..Default::default()
                    },
                ),
                take(
                    10,
                    1,
                    0,
                    Transition {
                        hold_us: Some(10),
                        repeat: Repeat::Forever,
                        ..Default::default()
                    },
                ),
            ],
            &[resource()],
            Limits::default(),
        )
        .unwrap();
        assert_eq!(visible(&schedule, 99), [(1, 0.5)]);
        assert!(schedule.frame_at(Some(100)).is_none());
    }

    #[test]
    fn malformed_events_and_exceeded_budgets_fail_before_playback() {
        let valid = take(0, 1, 0, Transition::default());
        let compile =
            |events: &[Event], limits| ClipSchedule::compile(100, events, &[resource()], limits);
        assert!(compile(&[valid.clone(), valid.clone()], Limits::default()).is_err());
        assert!(compile(&[take(-1, 1, 0, Transition::default())], Limits::default()).is_err());
        assert!(compile(&[take(100, 1, 0, Transition::default())], Limits::default()).is_err());
        assert!(
            compile(
                &[take(5, 1, 0, Transition::default()), valid.clone()],
                Limits::default()
            )
            .is_err()
        );
        for limits in [
            Limits {
                events: 0,
                ..Default::default()
            },
            Limits {
                layers: 0,
                ..Default::default()
            },
            Limits {
                resources: 0,
                ..Default::default()
            },
            Limits {
                retained_layers: 0,
                ..Default::default()
            },
        ] {
            assert!(compile(std::slice::from_ref(&valid), limits).is_err());
        }
        assert!(ClipSchedule::compile(100, &[valid], &[], Limits::default()).is_err());
    }

    #[test]
    fn leases_live_exactly_as_long_as_schedule_and_queued_snapshots() {
        let resource = resource();
        let weak = Arc::downgrade(&resource);
        let schedule = ClipSchedule::compile(
            100,
            &[take(0, 1, 0, Transition::default())],
            std::slice::from_ref(&resource),
            Limits::default(),
        )
        .unwrap();
        let queued = schedule.frame_at(Some(1)).unwrap().scene.clone();
        drop(resource);
        drop(schedule);
        assert!(weak.upgrade().is_some());
        drop(queued);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn visibility_is_reversible_and_clear_removes_only_its_layer() {
        let schedule = ClipSchedule::compile(
            100,
            &[
                take(0, 1, 0, Transition::default()),
                take(0, 2, 0, Transition::default()),
                Event {
                    at_us: 10,
                    action: Action::Visibility {
                        layer: LayerId(1),
                        visible: false,
                    },
                },
                Event {
                    at_us: 20,
                    action: Action::Visibility {
                        layer: LayerId(1),
                        visible: true,
                    },
                },
                Event {
                    at_us: 30,
                    action: Action::Clear(LayerId(2)),
                },
            ],
            &[resource()],
            Limits::default(),
        )
        .unwrap();
        assert_eq!(visible(&schedule, 10), [(2, 0.5)]);
        assert_eq!(visible(&schedule, 20), [(1, 0.5), (2, 0.5)]);
        assert_eq!(visible(&schedule, 30), [(1, 0.5)]);
        assert_eq!(visible(&schedule, 0), [(1, 0.5), (2, 0.5)]);
    }

    #[test]
    fn empty_schedule_is_inert_and_duplicate_resources_are_rejected() {
        let empty = ClipSchedule::compile(100, &[], &[], Limits::default()).unwrap();
        assert!(empty.frame_at(Some(1)).is_none());
        assert!(ClipSchedule::compile(0, &[], &[], Limits::default()).is_err());
        let first = resource();
        let replacement = Resource {
            key: ResourceKey {
                id: first.key.id,
                revision: 2,
            },
            descriptor: first.descriptor,
        };
        assert!(
            ClipSchedule::compile(100, &[], &[first, Arc::new(replacement)], Limits::default())
                .is_err()
        );
    }
}
