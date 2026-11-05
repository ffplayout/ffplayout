//! Paced graphics soak/latency probe; run each configuration in a fresh process.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use clap::Parser;
use ff_graphics::{
    ffmpeg::{
        scene::{CpuScene, SceneScaleCache},
        sources::animation::AnimationOptions,
    },
    playback::{Repeat, Transition},
    preparation::PreparationService,
    scene::{Command, LayerId, LayerSpec, SceneController, Scope},
    source::Completion,
    surface::{ColorEncoding, ResourceKey},
};
use ffmpeg_next::{format::Pixel, frame};

struct CountingAllocator;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

// Instrument Rust allocations in all threads, including workers. FFmpeg's native
// allocations are deliberately excluded; getrusage reports process-wide peak RSS.
// SAFETY: every operation forwards its original pointer/layout to System;
// instrumentation only updates independent atomic counters.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(size as u64, Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Parser)]
struct Options {
    #[arg(long, hide = true)]
    bench: bool,
    #[arg(long, default_value_t = 1920)]
    width: u32,
    #[arg(long, default_value_t = 1080)]
    height: u32,
    #[arg(long, default_value_t = 25)]
    fps: u32,
    #[arg(long, default_value_t = 3)]
    layers: usize,
    #[arg(long, default_value_t = 10)]
    seconds: u32,
    /// Run without real-time sleeps for a throughput comparison.
    #[arg(long)]
    unpaced: bool,
}

#[cfg(unix)]
fn usage() -> Result<(f64, f64)> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage writes a complete rusage to valid output storage.
    ensure!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0,
        "getrusage failed"
    );
    // SAFETY: the successful call above initialized the output.
    let usage = unsafe { usage.assume_init() };
    let cpu = usage.ru_utime.tv_sec as f64
        + usage.ru_stime.tv_sec as f64
        + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1_000_000.0;
    let rss = usage.ru_maxrss as f64;
    let mib = if cfg!(target_os = "macos") {
        rss / 1_048_576.0
    } else {
        rss / 1024.0
    };

    Ok((cpu, mib))
}

#[cfg(not(unix))]
fn usage() -> Result<(f64, f64)> {
    Ok((f64::NAN, f64::NAN))
}

fn main() -> Result<()> {
    let options = Options::parse();
    ensure!(
        options.width > 0
            && options.height > 0
            && options.width <= 8192
            && options.height <= 8192
            && options.width.is_multiple_of(2)
            && options.height.is_multiple_of(2),
        "invalid canvas"
    );
    ensure!(
        options.layers <= 16
            && (1..=120).contains(&options.fps)
            && (1..=86_400).contains(&options.seconds),
        "invalid layer count/fps/duration"
    );
    ffmpeg_next::init()?;
    let service = PreparationService::default();
    let budget = service.budget();
    let mut cpu = CpuScene::new(16, 128 * 1024 * 1024).with_preparation(&service);
    let mut controller = SceneController::new_local(16, 16);
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage/media_images");

    for index in 0..options.layers {
        // Mix variable-delay cached GIFs and independently decoded video sources.
        let file = if index.is_multiple_of(2) {
            "animated-logo.gif"
        } else {
            "animated-logo-video.mkv"
        };
        let resource = cpu.prepare_animation(
            ResourceKey {
                id: index as u64,
                revision: index as u64 + 1,
            },
            &root.join(file),
            AnimationOptions {
                high_quality: true,
                size: Some((320, 180)),
                repeat: Repeat::Forever,
                completion: Completion::Hide,
            },
            || false,
        )?;
        let layer = LayerSpec {
            id: LayerId(index as u64),
            resource: resource.key,
            z: index as i32,
            scope: Scope::default(),
            canvas: (options.width, options.height),
            position: (index as i32 * 80, index as i32 * 40),
            opacity: 0.8,
        };
        let outcomes = controller.apply_at_frame(
            0,
            [
                Command::Prepare(resource.key),
                Command::Ready(resource),
                Command::Take {
                    layer,
                    transition: Transition {
                        fade_in_us: 200_000,
                        hold_us: Some(2_000_000),
                        fade_out_us: 300_000,
                        pause_us: 500_000,
                        repeat: Repeat::Forever,
                    },
                },
            ],
        );
        ensure!(
            outcomes.iter().all(|ack| ack.outcome.is_ok()),
            "scene rejected"
        );
    }

    let mut target = frame::Video::new(Pixel::YUV420P, options.width, options.height);
    let mut cache = SceneScaleCache::default();
    let frames = u64::from(options.seconds) * u64::from(options.fps);
    // Touch a bounded rolling sample window before measuring RSS/allocations.
    let mut samples = vec![f64::NAN; frames.min(7200) as usize];
    let mut maximum_ms = 0.0_f64;
    let mut max_budget = (0, 0, 0);
    let mut late = 0;
    let mut unavailable_layers = 0;
    let mut blended_layers = 0;
    // Warm native kernels and decoders, excluded from reported allocation/CPU window.
    for frame in 0..options.fps {
        let time = i64::from(frame) * 1_000_000 / i64::from(options.fps);
        black_box(cpu.blend_scaled(
            &controller.advance(time),
            time,
            Scope::default(),
            &mut target,
            ColorEncoding::Bt601Limited,
            &mut cache,
        ));
        thread::sleep(Duration::from_millis(1));
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let bytes = ALLOCATED_BYTES.load(Ordering::Relaxed);
    let (cpu_start, _) = usage()?;
    let start = Instant::now();

    for index in 0..frames {
        // Model an incoming writable frame. Reset cost is excluded from blend
        // latency but included in process CPU, equally for the zero-layer baseline.
        for plane in 0..3 {
            target
                .data_mut(plane)
                .fill(if plane == 0 { 16 } else { 128 });
        }
        let time = 1_000_000 + (index * 1_000_000 / u64::from(options.fps)) as i64;
        let frame_start = Instant::now();
        let scene = controller.advance(time);
        let expected_layers = scene.evaluate(time, Scope::default()).count();
        let report = cpu.blend_scaled(
            &scene,
            time,
            Scope::default(),
            &mut target,
            ColorEncoding::Bt601Limited,
            &mut cache,
        );
        let elapsed_ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        let slot = index as usize % samples.len();
        samples[slot] = elapsed_ms;
        maximum_ms = maximum_ms.max(elapsed_ms);
        ensure!(
            report.missing == 0 && report.unsupported == 0,
            "composition rejected layers"
        );
        unavailable_layers += expected_layers.saturating_sub(report.blended);
        blended_layers += report.blended;
        black_box(&target);
        let used = budget.usage();
        max_budget = (
            max_budget.0.max(used.0),
            max_budget.1.max(used.1),
            max_budget.2.max(used.2),
        );
        if index > 0 && index % (u64::from(options.fps) * 60) == 0 {
            let (cpu_now, rss_now) = usage()?;
            println!(
                "progress_s={:.0} cpu_s={:.3} peak_rss_mib={rss_now:.2} budget_bytes={} leases={} jobs={}",
                start.elapsed().as_secs_f64(),
                cpu_now - cpu_start,
                used.0,
                used.1,
                used.2
            );
        }
        let deadline = Duration::from_secs_f64((index + 1) as f64 / f64::from(options.fps));
        if !options.unpaced {
            if let Some(wait) = deadline.checked_sub(start.elapsed()) {
                thread::sleep(wait);
            } else {
                late += 1;
            }
        }
    }
    let wall = start.elapsed().as_secs_f64();
    let (cpu_end, rss) = usage()?;
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) - allocations;
    let bytes = ALLOCATED_BYTES.load(Ordering::Relaxed) - bytes;
    samples.sort_by(f64::total_cmp);
    println!(
        "canvas={}x{} fps={} layers={} frames={} paced={} wall_s={wall:.3} cpu_percent={:.2} frame_p50_ms={:.4} frame_p99_ms={:.4} frame_max_ms={:.4} late_frames={} unavailable_layer_frames={} blended_layer_frames={} peak_rss_mib={rss:.2} rust_allocations_per_frame={:.3} rust_allocated_bytes_per_frame={:.1} max_budget_bytes={} max_leases={} max_jobs={}",
        options.width,
        options.height,
        options.fps,
        options.layers,
        frames,
        !options.unpaced,
        (cpu_end - cpu_start) / wall * 100.0,
        samples[samples.len() / 2],
        samples[(samples.len() - 1) * 99 / 100],
        maximum_ms,
        late,
        unavailable_layers,
        blended_layers,
        allocations as f64 / frames as f64,
        bytes as f64 / frames as f64,
        max_budget.0,
        max_budget.1,
        max_budget.2
    );
    cpu.stop_streams();
    ensure!(
        service.join(Duration::from_secs(5)),
        "media workers did not join"
    );
    drop(controller);
    cache.clear();
    cpu.retire();
    ensure!(
        budget.usage() == (0, 0, 0),
        "resources retained after cleanup: {:?}",
        budget.usage()
    );
    ensure!(
        service.shutdown(Duration::from_secs(5)),
        "preparation worker did not stop"
    );

    Ok(())
}
