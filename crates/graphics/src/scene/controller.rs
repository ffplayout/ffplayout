//! Bounded command admission and publication at program-frame boundaries.
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
};

use crate::{
    playback::{Presentation, Transition},
    surface::{Resource, ResourceKey},
};

#[cfg(test)]
use super::Scope;
use super::{Layer, LayerId, LayerSpec, SceneRevision};

#[derive(Clone, Debug)]
pub enum Command {
    /// Reserves one bounded preparation slot; it does not start a worker or do I/O.
    Prepare(ResourceKey),
    Ready(Arc<Resource>),
    Failed(ResourceKey),
    Take {
        layer: LayerSpec,
        transition: Transition,
    },
    /// Updates layout/opacity without restarting playback. Resource replacement
    /// requires an explicit clear/take; crossfading sources is not implicit.
    Update(LayerSpec),
    Stop(LayerId),
    Visibility {
        layer: LayerId,
        visible: bool,
    },
    Clear(LayerId),
    ClearAll,
    Release(ResourceKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    Budget,
    Stale,
    NotReady,
    AlreadyActive,
    Invalid,
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Acknowledgement {
    pub scene_revision: u64,
    pub outcome: Result<(), Rejection>,
}

pub type Receipt = Receiver<Acknowledgement>;

struct Envelope {
    command: Command,
    reply: SyncSender<Acknowledgement>,
}

#[derive(Clone)]
pub struct SceneControl {
    sender: SyncSender<Envelope>,
}

#[derive(Debug)]
pub enum SubmitError {
    Full(Command),
    Closed(Command),
}

impl SceneControl {
    /// Control-side operation; queue saturation never waits for the video thread.
    pub fn submit(&self, command: Command) -> Result<Receipt, SubmitError> {
        let (reply, receipt) = mpsc::sync_channel(1);
        match self.sender.try_send(Envelope { command, reply }) {
            Ok(()) => Ok(receipt),
            Err(TrySendError::Full(envelope)) => Err(SubmitError::Full(envelope.command)),
            Err(TrySendError::Disconnected(envelope)) => Err(SubmitError::Closed(envelope.command)),
        }
    }
}

struct Slot {
    key: ResourceKey,
    resource: Option<Arc<Resource>>,
    failed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceStatus {
    Loading,
    Ready,
    Active,
    Failed,
}

pub struct SceneController {
    receiver: Option<Receiver<Envelope>>,
    max_commands: usize,
    max_layers: usize,
    max_resources: usize,
    resources: BTreeMap<u64, Slot>,
    layers: BTreeMap<LayerId, Layer>,
    snapshot: SceneRevision,
    last_time: Option<i64>,
    last_preparation: Option<u64>,
}

impl SceneController {
    pub fn new(
        queue_capacity: usize,
        max_layers: usize,
        max_resources: usize,
    ) -> (SceneControl, Self) {
        let capacity = queue_capacity.max(1);
        let (sender, receiver) = mpsc::sync_channel(capacity);

        (
            SceneControl { sender },
            Self::with_receiver(Some(receiver), capacity, max_layers, max_resources),
        )
    }

    /// A single-owner driver publishes local updates at frame boundaries. It
    /// needs neither a control queue nor per-command receipt channels.
    pub fn new_local(max_layers: usize, max_resources: usize) -> Self {
        Self::with_receiver(None, 0, max_layers, max_resources)
    }

    fn with_receiver(
        receiver: Option<Receiver<Envelope>>,
        max_commands: usize,
        max_layers: usize,
        max_resources: usize,
    ) -> Self {
        Self {
            receiver,
            max_commands,
            max_layers,
            max_resources,
            resources: BTreeMap::new(),
            layers: BTreeMap::new(),
            snapshot: SceneRevision::default(),
            last_time: None,
            last_preparation: None,
        }
    }

    pub fn resource_status(&self, key: ResourceKey) -> Option<ResourceStatus> {
        if self.layers.values().any(|layer| layer.spec.resource == key) {
            return Some(ResourceStatus::Active);
        }
        let slot = self.resources.get(&key.id).filter(|slot| slot.key == key)?;

        Some(if slot.failed {
            ResourceStatus::Failed
        } else if slot.resource.is_some() {
            ResourceStatus::Ready
        } else {
            ResourceStatus::Loading
        })
    }

    /// Single-owner adapter path, called only at a program-frame boundary.
    /// Previously queued commands are handled first, including stale-epoch
    /// rejection after a clock rewind. Local results are acknowledged together
    /// after publication, without allocating channels/receipts for each command.
    /// Like queued commands, commands in the batch are individually accepted or
    /// rejected in order; this is not an all-or-nothing transaction.
    pub fn apply_at_frame<const N: usize>(
        &mut self,
        program_us: i64,
        commands: [Command; N],
    ) -> [Acknowledgement; N] {
        self.advance(program_us);
        let outcomes = commands.map(|command| self.apply(command, program_us));
        if outcomes.iter().any(Result::is_ok) {
            self.publish();
        }

        outcomes.map(|outcome| Acknowledgement {
            scene_revision: self.snapshot.number,
            outcome,
        })
    }

    /// Returns a shared immutable revision. Unchanged frames clone only its Arc.
    /// A backwards program clock starts a new epoch and clears pending/active state.
    pub fn advance(&mut self, program_us: i64) -> SceneRevision {
        let reset = self.last_time.is_some_and(|last| program_us < last);
        self.last_time = Some(program_us);
        let mut changed = reset;
        if reset {
            self.layers.clear();
            self.resources.clear();
        }
        let before = self.layers.len();
        self.layers
            .retain(|_, layer| layer.presentation.opacity(program_us).is_some());
        changed |= before != self.layers.len();
        let mut replies = Vec::new();
        // One bounded batch per frame also prevents a busy producer starving video.
        for _ in 0..self.max_commands {
            let Some(receiver) = &self.receiver else {
                break;
            };
            let Ok(envelope) = receiver.try_recv() else {
                break;
            };
            let outcome = if reset {
                Err(Rejection::Stale)
            } else {
                self.apply(envelope.command, program_us)
            };
            changed |= outcome.is_ok();
            replies.push((envelope.reply, outcome));
        }

        if changed {
            self.publish();
        }

        for (reply, outcome) in replies {
            let _ = reply.try_send(Acknowledgement {
                scene_revision: self.snapshot.number,
                outcome,
            });
        }

        self.snapshot.clone()
    }

    fn publish(&mut self) {
        let mut layers = self.layers.values().cloned().collect::<Vec<_>>();
        layers.sort_by_key(|layer| (layer.spec.z, layer.spec.id));
        self.snapshot = SceneRevision {
            number: self.snapshot.number.saturating_add(1),
            layers: layers.into(),
        };
    }

    fn apply(&mut self, command: Command, now: i64) -> Result<(), Rejection> {
        match command {
            Command::Prepare(key) => {
                // Revisions are globally increasing preparation request IDs for
                // this controller, including across clear/release/clock resets.
                if self
                    .last_preparation
                    .is_some_and(|last| key.revision <= last)
                {
                    return Err(Rejection::Stale);
                }

                if let Some(slot) = self.resources.get(&key.id) {
                    if key.revision <= slot.key.revision {
                        return Err(Rejection::Stale);
                    }
                } else if self.resources.len() >= self.max_resources {
                    return Err(Rejection::Budget);
                }
                self.last_preparation = Some(key.revision);
                self.resources.insert(
                    key.id,
                    Slot {
                        key,
                        resource: None,
                        failed: false,
                    },
                );
            }
            Command::Ready(resource) => {
                resource
                    .descriptor
                    .validate()
                    .map_err(|_| Rejection::Invalid)?;
                let slot = self
                    .resources
                    .get_mut(&resource.key.id)
                    .filter(|slot| {
                        slot.key == resource.key && slot.resource.is_none() && !slot.failed
                    })
                    .ok_or(Rejection::Stale)?;
                slot.resource = Some(resource);
            }
            Command::Failed(key) => {
                let slot = self
                    .resources
                    .get_mut(&key.id)
                    .filter(|slot| slot.key == key && slot.resource.is_none())
                    .ok_or(Rejection::Stale)?;
                slot.failed = true;
            }
            Command::Take { layer, transition } => {
                if self.layers.contains_key(&layer.id) {
                    return Err(Rejection::AlreadyActive);
                }
                if self.layers.len() >= self.max_layers {
                    return Err(Rejection::Budget);
                }
                if layer.canvas.0 == 0
                    || layer.canvas.1 == 0
                    || !layer.opacity.is_finite()
                    || !(0.0..=1.0).contains(&layer.opacity)
                {
                    return Err(Rejection::Invalid);
                }
                let resource = self
                    .resources
                    .get(&layer.resource.id)
                    .filter(|slot| slot.key == layer.resource)
                    .and_then(|slot| slot.resource.clone())
                    .ok_or(Rejection::NotReady)?;
                let presentation =
                    Presentation::new(now, transition).map_err(|_| Rejection::Invalid)?;
                self.layers.insert(
                    layer.id,
                    Layer {
                        spec: layer,
                        resource,
                        presentation,
                        visible: true,
                    },
                );
            }
            Command::Update(spec) => {
                if spec.canvas.0 == 0
                    || spec.canvas.1 == 0
                    || !spec.opacity.is_finite()
                    || !(0.0..=1.0).contains(&spec.opacity)
                {
                    return Err(Rejection::Invalid);
                }
                let layer = self.layers.get_mut(&spec.id).ok_or(Rejection::Missing)?;
                if spec.resource != layer.spec.resource {
                    return Err(Rejection::Invalid);
                }
                layer.spec = spec;
            }
            Command::Stop(id) => self
                .layers
                .get_mut(&id)
                .ok_or(Rejection::Missing)?
                .presentation
                .stop(now),
            Command::Visibility { layer, visible } => {
                self.layers
                    .get_mut(&layer)
                    .ok_or(Rejection::Missing)?
                    .visible = visible;
            }
            Command::Clear(id) => {
                self.layers.remove(&id).ok_or(Rejection::Missing)?;
            }
            Command::ClearAll => {
                self.layers.clear();
                self.resources.clear();
            }
            Command::Release(key) => {
                let current = self
                    .resources
                    .get(&key.id)
                    .is_some_and(|slot| slot.key == key);
                let before = self.layers.len();
                self.layers.retain(|_, layer| layer.spec.resource != key);
                if current {
                    self.resources.remove(&key.id);
                } else if before == self.layers.len() {
                    return Err(Rejection::Stale);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::{Alpha, ColorEncoding, PixelLayout, SurfaceDescriptor, SurfaceStore};

    fn spec(id: u64, resource: ResourceKey, z: i32) -> LayerSpec {
        LayerSpec {
            id: LayerId(id),
            resource,
            z,
            scope: Scope::default(),
            canvas: (1280, 720),
            position: (10, 20),
            opacity: 1.0,
        }
    }

    fn ready(
        control: &SceneControl,
        controller: &mut SceneController,
        id: u64,
        revision: u64,
    ) -> Arc<Resource> {
        let key = ResourceKey { id, revision };
        let descriptor = SurfaceDescriptor {
            width: 2,
            height: 2,
            layout: PixelLayout::Rgba8,
            alpha: Alpha::Straight,
            color: ColorEncoding::Srgb,
            storage_bytes: 16,
            metadata: None,
        };
        let mut store = SurfaceStore::new(1, 16);
        let resource = store.insert(key, descriptor, Arc::new([0_u8; 16])).unwrap();
        let receipt = control.submit(Command::Prepare(key)).unwrap();
        controller.advance(0);
        assert_eq!(receipt.recv().unwrap().outcome, Ok(()));
        let receipt = control.submit(Command::Ready(resource.clone())).unwrap();
        controller.advance(0);
        assert_eq!(receipt.recv().unwrap().outcome, Ok(()));

        resource
    }

    #[test]
    fn local_batches_publish_together_and_unchanged_frames_reuse_the_revision() {
        let mut controller = SceneController::new_local(1, 1);
        let key = ResourceKey { id: 1, revision: 1 };
        let resource = Arc::new(Resource {
            key,
            descriptor: SurfaceDescriptor {
                width: 2,
                height: 2,
                layout: PixelLayout::Rgba8,
                alpha: Alpha::Straight,
                color: ColorEncoding::Srgb,
                storage_bytes: 16,
                metadata: None,
            },
        });
        let acknowledgements = controller.apply_at_frame(
            100,
            [
                Command::Prepare(key),
                Command::Ready(resource),
                Command::Take {
                    layer: spec(1, key, 0),
                    transition: Transition::default(),
                },
            ],
        );
        assert!(
            acknowledgements
                .iter()
                .all(|ack| ack.outcome.is_ok() && ack.scene_revision == 1)
        );
        let first = controller.advance(100);
        let later = controller.advance(200);
        assert!(Arc::ptr_eq(&first.layers, &later.layers));
        assert_eq!(
            controller.resource_status(key),
            Some(ResourceStatus::Active)
        );
        controller.advance(50);
        let [stale] = controller.apply_at_frame(50, [Command::Prepare(key)]);
        assert_eq!(stale.outcome, Err(Rejection::Stale));
        assert!(controller.advance(50).layers.is_empty());
    }

    #[test]
    fn active_resource_remains_inspectable_while_its_replacement_loads_or_fails() {
        let (control, mut controller) = SceneController::new(4, 1, 1);
        let resource = ready(&control, &mut controller, 1, 1);
        controller.apply_at_frame(
            10,
            [Command::Take {
                layer: spec(1, resource.key, 0),
                transition: Transition::default(),
            }],
        );
        let replacement = ResourceKey { id: 1, revision: 2 };
        controller.apply_at_frame(20, [Command::Prepare(replacement)]);
        assert_eq!(
            controller.resource_status(resource.key),
            Some(ResourceStatus::Active)
        );
        assert_eq!(
            controller.resource_status(replacement),
            Some(ResourceStatus::Loading)
        );
        controller.apply_at_frame(30, [Command::Failed(replacement)]);
        assert_eq!(
            controller.resource_status(resource.key),
            Some(ResourceStatus::Active)
        );
        assert_eq!(
            controller.resource_status(replacement),
            Some(ResourceStatus::Failed)
        );
        assert_eq!(controller.advance(30).layers.len(), 1);
        controller.apply_at_frame(40, [Command::ClearAll]);
        let [late] = controller.apply_at_frame(50, [Command::Ready(resource)]);
        assert_eq!(late.outcome, Err(Rejection::Stale));
    }

    #[test]
    fn local_boundary_updates_reject_queued_old_epoch_results_before_applying_new_work() {
        let (control, mut controller) = SceneController::new(2, 1, 1);
        controller.advance(100);
        let stale = control
            .submit(Command::Prepare(ResourceKey { id: 1, revision: 1 }))
            .unwrap();
        let key = ResourceKey { id: 1, revision: 2 };
        let [fresh] = controller.apply_at_frame(50, [Command::Prepare(key)]);
        assert_eq!(stale.recv().unwrap().outcome, Err(Rejection::Stale));
        assert_eq!(fresh.outcome, Ok(()));
        assert_eq!(
            controller.resource_status(key),
            Some(ResourceStatus::Loading)
        );
    }

    #[test]
    fn frame_boundaries_publish_immutable_sorted_snapshots_and_reuse_unchanged_storage() {
        let (control, mut controller) = SceneController::new(8, 8, 8);
        let resource = ready(&control, &mut controller, 1, 1);
        let mut receipts = Vec::new();
        for (id, z) in [(3, 10), (2, 5), (1, 10)] {
            receipts.push(
                control
                    .submit(Command::Take {
                        layer: spec(id, resource.key, z),
                        transition: Transition::default(),
                    })
                    .unwrap(),
            );
        }
        assert!(controller.snapshot.layers.is_empty());
        let original = controller.advance(100);
        assert_eq!(
            original
                .layers
                .iter()
                .map(|layer| layer.spec.id.0)
                .collect::<Vec<_>>(),
            [2, 1, 3]
        );
        for receipt in receipts {
            assert_eq!(
                receipt.recv().unwrap(),
                Acknowledgement {
                    scene_revision: original.number,
                    outcome: Ok(())
                }
            );
        }
        let next = controller.advance(200);
        assert!(Arc::ptr_eq(&original.layers, &next.layers));
        control.submit(Command::Clear(LayerId(1))).unwrap();
        let cleared = controller.advance(300);
        assert_eq!(original.layers.len(), 3);
        assert_eq!(cleared.layers.len(), 2);
        assert_eq!(original.evaluate(300, Scope(99)).count(), 0);
    }

    #[test]
    fn bounded_commands_and_resources_reject_overload_without_blocking() {
        let (control, mut controller) = SceneController::new(1, 1, 1);
        let key = ResourceKey { id: 1, revision: 1 };
        let receipt = control.submit(Command::Prepare(key)).unwrap();
        assert!(matches!(
            control.submit(Command::ClearAll),
            Err(SubmitError::Full(_))
        ));
        controller.advance(0);
        assert_eq!(receipt.recv().unwrap().outcome, Ok(()));
        let receipt = control
            .submit(Command::Prepare(ResourceKey { id: 2, revision: 2 }))
            .unwrap();
        controller.advance(0);
        assert_eq!(receipt.recv().unwrap().outcome, Err(Rejection::Budget));
        drop(controller);
        assert!(matches!(
            control.submit(Command::ClearAll),
            Err(SubmitError::Closed(_))
        ));
    }

    #[test]
    fn cleared_or_replaced_resources_cannot_be_reactivated_by_stale_completions() {
        let (control, mut controller) = SceneController::new(8, 8, 2);
        let resource = ready(&control, &mut controller, 1, 1);
        control.submit(Command::ClearAll).unwrap();
        controller.advance(0);
        let stale = control.submit(Command::Ready(resource.clone())).unwrap();
        let reused = control.submit(Command::Prepare(resource.key)).unwrap();
        controller.advance(0);
        assert_eq!(stale.recv().unwrap().outcome, Err(Rejection::Stale));
        assert_eq!(reused.recv().unwrap().outcome, Err(Rejection::Stale));
        control
            .submit(Command::Prepare(ResourceKey { id: 1, revision: 2 }))
            .unwrap();
        let stale = control.submit(Command::Failed(resource.key)).unwrap();
        controller.advance(0);
        assert_eq!(stale.recv().unwrap().outcome, Err(Rejection::Stale));
        assert_eq!(
            controller.resource_status(ResourceKey { id: 1, revision: 2 }),
            Some(ResourceStatus::Loading)
        );
    }

    #[test]
    fn not_ready_failed_retakes_and_invalid_layers_are_contained() {
        let (control, mut controller) = SceneController::new(8, 1, 2);
        let key = ResourceKey { id: 1, revision: 1 };
        control.submit(Command::Prepare(key)).unwrap();
        let not_ready = control
            .submit(Command::Take {
                layer: spec(1, key, 0),
                transition: Transition::default(),
            })
            .unwrap();
        controller.advance(0);
        assert_eq!(not_ready.recv().unwrap().outcome, Err(Rejection::NotReady));
        control.submit(Command::Failed(key)).unwrap();
        controller.advance(0);
        assert_eq!(
            controller.resource_status(key),
            Some(ResourceStatus::Failed)
        );
        let resource = ready(&control, &mut controller, 1, 2);
        let mut invalid = spec(1, resource.key, 0);
        invalid.opacity = f64::NAN;
        let rejected = control
            .submit(Command::Take {
                layer: invalid,
                transition: Transition::default(),
            })
            .unwrap();
        control
            .submit(Command::Take {
                layer: spec(1, resource.key, 0),
                transition: Transition::default(),
            })
            .unwrap();
        controller.advance(0);
        assert_eq!(rejected.recv().unwrap().outcome, Err(Rejection::Invalid));
        let retake = control
            .submit(Command::Take {
                layer: spec(1, resource.key, 0),
                transition: Transition::default(),
            })
            .unwrap();
        let overload = control
            .submit(Command::Take {
                layer: spec(2, resource.key, 0),
                transition: Transition::default(),
            })
            .unwrap();
        controller.advance(0);
        assert_eq!(
            retake.recv().unwrap().outcome,
            Err(Rejection::AlreadyActive)
        );
        assert_eq!(overload.recv().unwrap().outcome, Err(Rejection::Budget));
    }

    #[test]
    fn completed_stop_releases_active_lease_but_queued_snapshots_keep_it_alive() {
        let (control, mut controller) = SceneController::new(8, 8, 8);
        let resource = ready(&control, &mut controller, 1, 1);
        control
            .submit(Command::Take {
                layer: spec(1, resource.key, 0),
                transition: Transition {
                    fade_in_us: 100,
                    fade_out_us: 100,
                    ..Default::default()
                },
            })
            .unwrap();
        controller.advance(0);
        control.submit(Command::Stop(LayerId(1))).unwrap();
        let stopped = controller.advance(50);
        assert_eq!(
            stopped
                .evaluate(100, Scope::default())
                .next()
                .unwrap()
                .content
                .opacity,
            0.25
        );
        assert!(controller.advance(150).layers.is_empty());
        assert_eq!(stopped.layers.len(), 1);
        assert_eq!(
            controller.resource_status(resource.key),
            Some(ResourceStatus::Ready)
        );
        control
            .submit(Command::Prepare(ResourceKey { id: 1, revision: 2 }))
            .unwrap();
        controller.advance(200);
        let weak = Arc::downgrade(&resource);
        drop(resource);
        assert!(weak.upgrade().is_some());
        drop(stopped);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn updates_and_visibility_preserve_the_transition_and_old_snapshots() {
        let (control, mut controller) = SceneController::new(8, 8, 8);
        let resource = ready(&control, &mut controller, 1, 1);
        let original = spec(1, resource.key, 0);
        control
            .submit(Command::Take {
                layer: original,
                transition: Transition {
                    fade_in_us: 100,
                    ..Default::default()
                },
            })
            .unwrap();
        let first = controller.advance(0);
        let updated = LayerSpec {
            opacity: 0.5,
            position: (50, 60),
            ..original
        };
        control.submit(Command::Update(updated)).unwrap();
        let second = controller.advance(50);
        assert_eq!(
            second
                .evaluate(50, Scope::default())
                .next()
                .unwrap()
                .content
                .opacity,
            0.25
        );
        assert_eq!(first.layers[0].spec.position, (10, 20));
        assert_eq!(second.layers[0].spec.position, (50, 60));
        control
            .submit(Command::Visibility {
                layer: original.id,
                visible: false,
            })
            .unwrap();
        assert_eq!(
            controller
                .advance(60)
                .evaluate(60, Scope::default())
                .count(),
            0
        );
        control
            .submit(Command::Visibility {
                layer: original.id,
                visible: true,
            })
            .unwrap();
        assert_eq!(
            controller
                .advance(100)
                .evaluate(100, Scope::default())
                .next()
                .unwrap()
                .content
                .opacity,
            0.5
        );
    }

    #[test]
    fn releasing_old_active_revision_preserves_its_pending_replacement() {
        let (control, mut controller) = SceneController::new(8, 8, 8);
        let resource = ready(&control, &mut controller, 1, 1);
        control
            .submit(Command::Take {
                layer: spec(1, resource.key, 0),
                transition: Transition::default(),
            })
            .unwrap();
        controller.advance(0);
        let replacement = ResourceKey { id: 1, revision: 2 };
        control.submit(Command::Prepare(replacement)).unwrap();
        let released = control.submit(Command::Release(resource.key)).unwrap();
        assert!(controller.advance(1).layers.is_empty());
        assert_eq!(released.recv().unwrap().outcome, Ok(()));
        assert_eq!(
            controller.resource_status(replacement),
            Some(ResourceStatus::Loading)
        );
    }

    #[test]
    fn a_clock_reset_discards_queued_old_epoch_commands() {
        let (control, mut controller) = SceneController::new(8, 8, 8);
        controller.advance(100);
        let receipt = control
            .submit(Command::Prepare(ResourceKey { id: 1, revision: 1 }))
            .unwrap();
        assert!(controller.advance(0).layers.is_empty());
        assert_eq!(receipt.recv().unwrap().outcome, Err(Rejection::Stale));
    }
}
