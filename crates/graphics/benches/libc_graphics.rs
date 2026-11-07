//! Production graphics preparation and compositor kernels under either libc.
use std::hint::black_box;

use anyhow::Result;
use clap::Parser;
use ff_graphics::{
    ffmpeg::{
        overlay::{
            BlendExecution, OverlayFrame, alpha_bounds, blend_overlay_with_execution, chroma_alpha,
        },
        sources::preparation::{PreparationExecution, SourcePreparation},
    },
    surface::Alpha,
};
use ffmpeg_next::{
    format::Pixel,
    frame,
    util::color::{Range, Space},
};
use rayon::ThreadPoolBuilder;

#[path = "../../../benches/support/mod.rs"]
mod support;

const CASES: &[&str] = &[
    "prepare_1080_serial",
    "prepare_1080_parallel",
    "prepare_4k_serial",
    "prepare_4k_parallel",
    "prepare_1080_cold",
    "prepare_1080_premultiplied",
    "blend_1080_serial",
    "blend_1080_parallel",
    "blend_4k_serial",
    "blend_4k_parallel",
];

fn main() -> Result<()> {
    let args = support::Args::parse();

    let Some(case) = args.selected(CASES)? else {
        return Ok(());
    };

    ffmpeg_next::init()?;

    let (width, height) = if case.contains("4k") {
        (3840, 2160)
    } else {
        (1920, 1080)
    };
    let pool = ThreadPoolBuilder::new().num_threads(args.threads).build()?;
    let parallel = case.ends_with("parallel");

    if case.starts_with("prepare") {
        let mut source = frame::Video::new(Pixel::RGBA64LE, width, height);

        source.set_color_space(Space::RGB);
        source.set_color_range(Range::JPEG);

        let stride = source.stride(0);

        for y in 0..height as usize {
            for x in 0..width as usize {
                let alpha = ((x * 65_535) / (width as usize - 1)) as u16;

                for (channel, value) in [50_000_u16, 24_000, 12_000, alpha].into_iter().enumerate()
                {
                    let value = if case.contains("premultiplied") && channel < 3 {
                        ((u64::from(value) * u64::from(alpha) + 32767) / 65535) as u16
                    } else {
                        value
                    };

                    source.data_mut(0)[y * stride + x * 8 + channel * 2..][..2]
                        .copy_from_slice(&value.to_le_bytes());
                }
            }
        }

        let mut preparation = SourcePreparation::default();
        let execution = if parallel {
            PreparationExecution::Parallel(&pool)
        } else {
            PreparationExecution::Serial
        };

        support::measure(&args, &case, 1, || {
            if case.ends_with("cold") {
                preparation = SourcePreparation::default();
            }

            let output = preparation.prepare_with_execution(
                black_box(&source),
                None,
                if case.contains("premultiplied") {
                    Alpha::PremultipliedEncoded
                } else {
                    Alpha::Straight
                },
                (width / 2, height / 2),
                execution,
            )?;

            Ok(u64::from(black_box(output.data(0)[output.stride(0) + 100])))
        })?;
    } else {
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
        let mut target = frame::Video::new(Pixel::YUV420P, width, height);

        support::measure(&args, &case, 1, || {
            // Include the same deterministic reset in both variants. Allocation
            // and thread-pool construction are outside the steady-state loop.
            for plane in 0..3 {
                target.data_mut(plane).fill(70);
            }

            blend_overlay_with_execution(
                black_box(&mut target),
                overlay.as_ref(),
                0.8,
                if parallel {
                    BlendExecution::Parallel(&pool)
                } else {
                    BlendExecution::Serial
                },
            );

            Ok(u64::from(target.data(0)[100]))
        })?;
    }

    Ok(())
}
