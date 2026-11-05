//! One configuration per process: includes conversion/scaling/allocation, excludes source decoding.
use std::{hint::black_box, time::Instant};

use anyhow::{Result, ensure};
use clap::{Parser, ValueEnum};
use ff_graphics::{
    ffmpeg::sources::preparation::{PreparationExecution, SourcePreparation},
    surface::Alpha,
};
use ffmpeg_next::{
    frame,
    software::scaling,
    util::{
        color::{Range, Space},
        format::pixel::Pixel,
    },
};

#[path = "../examples/frame_blend/cpu_time.rs"]
mod cpu_time;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Mode {
    Legacy,
    Straight,
    Premultiplied,
}

#[derive(Parser)]
struct Args {
    #[arg(long, hide = true)]
    bench: bool,
    #[arg(long, default_value_t = 1920)]
    width: u32,
    #[arg(long, default_value_t = 1080)]
    height: u32,
    #[arg(long)]
    resize: bool,
    #[arg(long, value_enum, default_value_t = Mode::Straight)]
    mode: Mode,
    #[arg(long, default_value_t = 31)]
    samples: usize,
    #[arg(long, default_value = "adaptive")]
    execution: String,
    #[arg(long, default_value = "rgba16")]
    input: String,
}

#[cfg(unix)]
fn peak_rss_mib() -> Result<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes this correctly sized output on success.
    ensure!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0,
        "getrusage failed: {}",
        std::io::Error::last_os_error()
    );
    let usage = unsafe { usage.assume_init() };
    let bytes = usage.ru_maxrss as f64
        * if cfg!(target_os = "macos") {
            1.0
        } else {
            1024.0
        };

    Ok(bytes / (1024.0 * 1024.0))
}

#[cfg(not(unix))]
fn peak_rss_mib() -> Result<f64> {
    Ok(f64::NAN)
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.samples > 0
            && args.width >= 4
            && args.height >= 4
            && args.width <= 8192
            && args.height <= 8192
            && args.width.is_multiple_of(4)
            && args.height.is_multiple_of(4),
        "invalid benchmark dimensions or sample count"
    );
    let pool = if args.execution == "parallel" {
        Some(rayon::ThreadPoolBuilder::new().num_threads(4).build()?)
    } else {
        None
    };
    let execution = match args.execution.as_str() {
        "reference" => PreparationExecution::Reference,
        "serial" => PreparationExecution::Serial,
        "adaptive" => PreparationExecution::Adaptive,
        "parallel" => {
            PreparationExecution::Parallel(pool.as_ref().expect("parallel executor created"))
        }
        _ => anyhow::bail!("unknown execution mode"),
    };
    let size = if args.resize {
        (args.width / 2, args.height / 2)
    } else {
        (args.width, args.height)
    };
    let mut source = frame::Video::new(Pixel::RGBA64LE, args.width, args.height);
    source.set_color_space(Space::RGB);
    source.set_color_range(Range::JPEG);
    let stride = source.stride(0);

    for y in 0..args.height as usize {
        for x in 0..args.width as usize {
            let alpha = ((x * 65535) / (args.width as usize - 1)) as u16;

            for (channel, value) in [50000_u16, 24000, 12000, alpha].into_iter().enumerate() {
                let value = if matches!(args.mode, Mode::Premultiplied) && channel < 3 {
                    ((u64::from(value) * u64::from(alpha) + 32767) / 65535) as u16
                } else {
                    value
                };
                source.data_mut(0)
                    [y * stride + x * 8 + channel * 2..y * stride + x * 8 + channel * 2 + 2]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
    }

    let format = match args.input.as_str() {
        "rgba16" => Pixel::RGBA64LE,
        "rgba8" => Pixel::RGBA,
        "yuva8" => Pixel::YUVA420P,
        _ => anyhow::bail!("unknown input pixel format"),
    };

    if format != Pixel::RGBA64LE {
        let mut convert = scaling::Context::get(
            Pixel::RGBA64LE,
            args.width,
            args.height,
            format,
            args.width,
            args.height,
            scaling::flag::Flags::BILINEAR,
        )?;
        let mut converted = frame::Video::empty();
        convert.run(&source, &mut converted)?;
        converted.set_color_space(if format == Pixel::YUVA420P {
            Space::SMPTE170M
        } else {
            Space::RGB
        });
        converted.set_color_range(if format == Pixel::YUVA420P {
            Range::MPEG
        } else {
            Range::JPEG
        });
        source = converted;
    }

    let mut preparation = SourcePreparation::default();
    let mut legacy = scaling::Context::get(
        source.format(),
        args.width,
        args.height,
        Pixel::YUVA420P,
        size.0,
        size.1,
        scaling::flag::Flags::BILINEAR,
    )?;
    let mut wall = Vec::new();
    let mut cpu = Vec::new();

    for index in 0..args.samples + 3 {
        let cpu_start = cpu_time::process_cpu_seconds()?;
        let start = Instant::now();
        let output = if matches!(args.mode, Mode::Legacy) {
            let mut output = frame::Video::empty();
            legacy.run(black_box(&source), &mut output)?;
            output
        } else {
            preparation.prepare_with_execution(
                black_box(&source),
                None,
                if matches!(args.mode, Mode::Premultiplied) {
                    Alpha::PremultipliedEncoded
                } else {
                    Alpha::Straight
                },
                size,
                execution,
            )?
        };
        black_box(&output);
        drop(output);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        let cpu_elapsed = (cpu_time::process_cpu_seconds()? - cpu_start) * 1000.0;

        if index >= 3 {
            wall.push(elapsed);
            cpu.push(cpu_elapsed);
        }
    }

    wall.sort_by(f64::total_cmp);
    cpu.sort_by(f64::total_cmp);
    println!(
        "source={}x{} target={}x{} mode={:?} input={} execution={} samples={} wall_median_ms={:.3} wall_p95_ms={:.3} cpu_median_ms={:.3} peak_rss_mib={:.2}",
        args.width,
        args.height,
        size.0,
        size.1,
        args.mode,
        args.input,
        args.execution,
        args.samples,
        wall[wall.len() / 2],
        wall[((wall.len() - 1) * 95) / 100],
        cpu[cpu.len() / 2],
        peak_rss_mib()?
    );

    Ok(())
}
