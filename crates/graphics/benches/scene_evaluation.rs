//! Metadata-only scene overhead; no rasterization, conversion or blending.
use std::{hint::black_box, sync::Arc, time::Instant};

use ff_graphics::{
    playback::{Repeat, Transition},
    playlist::{Action, ClipSchedule, Event, Limits},
    scene::{Command, LayerId, LayerSpec, SceneController, Scope},
    surface::{Alpha, ColorEncoding, PixelLayout, ResourceKey, SurfaceDescriptor, SurfaceStore},
};

fn main() {
    for layers in [0, 3, 16, 64] {
        let mut store = SurfaceStore::new(1, 16);
        let resource = store
            .insert(
                ResourceKey { id: 1, revision: 1 },
                SurfaceDescriptor {
                    width: 2,
                    height: 2,
                    layout: PixelLayout::Rgba8,
                    alpha: Alpha::Straight,
                    color: ColorEncoding::Srgb,
                    storage_bytes: 16,
                },
                Arc::new([0_u8; 16]),
            )
            .unwrap();
        let (control, mut controller) = SceneController::new(128, 64, 1);
        control.submit(Command::Prepare(resource.key)).unwrap();
        control.submit(Command::Ready(resource.clone())).unwrap();
        for id in 0..layers {
            control
                .submit(Command::Take {
                    layer: LayerSpec {
                        id: LayerId(id),
                        resource: resource.key,
                        z: id as i32,
                        scope: Scope::default(),
                        canvas: (1920, 1080),
                        position: (0, 0),
                        opacity: 0.8,
                    },
                    transition: Transition {
                        fade_in_us: 500_000,
                        hold_us: Some(1_000_000),
                        fade_out_us: 500_000,
                        repeat: Repeat::Forever,
                        pause_us: 0,
                    },
                })
                .unwrap();
        }
        controller.advance(0);
        let iterations = 100_000_i64;
        let mut samples = Vec::new();
        for round in 0..7_i64 {
            let start = Instant::now();
            for frame in 0..iterations {
                let time = (round * iterations + frame) * 40_000;
                let scene = controller.advance(black_box(time));
                let total: f64 = scene
                    .evaluate(time, black_box(Scope::default()))
                    .map(|layer| layer.opacity)
                    .sum();
                black_box(total);
            }
            samples.push(start.elapsed().as_nanos() as f64 / iterations as f64);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "layers={layers} advance+evaluate median_ns/frame={:.1} samples={samples:?}",
            samples[3]
        );

        let events = (0..layers)
            .map(|id| Event {
                at_us: id as i64 * 40_000,
                action: Action::Take {
                    layer: LayerSpec {
                        id: LayerId(id),
                        resource: resource.key,
                        z: id as i32,
                        scope: Scope::default(),
                        canvas: (1920, 1080),
                        position: (0, 0),
                        opacity: 0.8,
                    },
                    transition: Transition {
                        fade_in_us: 500_000,
                        hold_us: Some(1_000_000),
                        fade_out_us: 500_000,
                        repeat: Repeat::Forever,
                        pause_us: 0,
                    },
                },
            })
            .collect::<Vec<_>>();
        let schedule = ClipSchedule::compile(
            iterations * 40_000,
            &events,
            &[resource],
            Limits {
                layers: 64,
                ..Default::default()
            },
        )
        .unwrap();
        let mut samples = Vec::new();
        for _ in 0..7 {
            let start = Instant::now();
            for frame in 0..iterations {
                let time = frame * 40_000;
                let total: f64 = schedule
                    .frame_at(black_box(Some(time)))
                    .map_or(0.0, |selected| {
                        selected
                            .scene
                            .evaluate(selected.position_us, black_box(Scope::default()))
                            .map(|layer| layer.opacity)
                            .sum()
                    });
                black_box(total);
            }
            samples.push(start.elapsed().as_nanos() as f64 / iterations as f64);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "layers={layers} schedule+evaluate median_ns/frame={:.1} samples={samples:?}",
            samples[3]
        );
    }
}
