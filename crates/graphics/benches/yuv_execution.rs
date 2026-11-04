//! Self-contained scheduling crossover benchmark, with aggregate process CPU time.
use std::{hint::black_box, time::Instant};

use anyhow::Result;
use ff_graphics::ffmpeg::overlay::{
    BlendExecution, OverlayFrame, alpha_bounds, blend_overlay_with_execution, chroma_alpha,
};
use ffmpeg_next::{frame, util::format::pixel::Pixel};
use rayon::ThreadPoolBuilder;

#[path = "../examples/frame_blend/cpu_time.rs"]
mod cpu_time;

const SAMPLES: usize = 501;

fn main() -> Result<()> {
    let pool = ThreadPoolBuilder::new().num_threads(4).build()?;
    println!(
        "501 interleaved samples; reset/preparation excluded; CPU = process user+system including workers."
    );
    println!("width height pixels backend wall_median_ms wall_p95_ms cpu_median_ms");
    for (width, height) in [
        (44, 44),
        (320, 180),
        (1280, 96),
        (1024, 256),
        (1024, 512),
        (1024, 1024),
        (1920, 1080),
        (2560, 1440),
        (3840, 2160),
    ] {
        let mut source = frame::Video::new(Pixel::YUVA420P, width, height);
        for plane in 0..4 {
            source
                .data_mut(plane)
                .fill(if plane == 3 { 179 } else { 90 });
        }
        let overlay = OverlayFrame {
            lease: None,
            bounds: alpha_bounds(&source),
            chroma_alpha: chroma_alpha(&source, width, height),
            frame: source,
            x: 0,
            y: 0,
            width,
            height,
            opacity: 255,
        };
        let mut target = frame::Video::new(Pixel::YUV420P, 3840, 2160);
        let mut wall: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
        let mut cpu: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::new());
        for sample in 0..SAMPLES + 5 {
            for index in 0..2 {
                let variant = (sample + index) % 2;
                for plane in 0..3 {
                    target.data_mut(plane).fill(70);
                }
                let execution = if variant == 0 {
                    BlendExecution::Serial
                } else {
                    BlendExecution::Parallel(&pool)
                };
                let cpu_start = cpu_time::process_cpu_seconds()?;
                let start = Instant::now();
                blend_overlay_with_execution(
                    black_box(&mut target),
                    overlay.as_ref(),
                    1.0,
                    execution,
                );
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let cpu_elapsed = (cpu_time::process_cpu_seconds()? - cpu_start) * 1000.0;
                if sample >= 5 {
                    wall[variant].push(elapsed);
                    cpu[variant].push(cpu_elapsed);
                }
                black_box(&target);
            }
        }
        for i in 0..2 {
            wall[i].sort_by(f64::total_cmp);
            cpu[i].sort_by(f64::total_cmp);
            println!(
                "{width} {height} {} {} {:.6} {:.6} {:.6}",
                width * height,
                if i == 0 { "serial" } else { "rayon4" },
                wall[i][SAMPLES / 2],
                wall[i][(SAMPLES - 1) * 95 / 100],
                cpu[i][SAMPLES / 2]
            );
        }
    }

    Ok(())
}
