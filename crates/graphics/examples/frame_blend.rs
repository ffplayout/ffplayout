//! CPU compositing laboratory. See docs/frame-blend.md for color assumptions,
//! timing boundaries and interpretation. This example does not change playout.
use std::{fs, hint::black_box, path::PathBuf, time::Instant};

use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use image::{
    ExtendedColorType, ImageEncoder,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use rayon::{ThreadPool, ThreadPoolBuilder};

#[path = "frame_blend/color.rs"]
mod color;
#[path = "frame_blend/cpu_time.rs"]
mod cpu_time;
#[path = "frame_blend/data.rs"]
mod data;
#[path = "frame_blend/kernels.rs"]
mod kernels;
#[path = "frame_blend/linear_simd.rs"]
mod linear_simd;
#[path = "frame_blend/paths.rs"]
mod paths;
#[path = "frame_blend/text_demo.rs"]
mod text_demo;
#[path = "frame_blend/video_input.rs"]
mod video_input;

use color::{Color, Matrix, Range, Rect, Transfer, quantize, source_rgb};
use data::Data;
use paths::{Scratch, blend, finish, prepare};

#[derive(Parser, Debug)]
#[command(about = "Compare premultiplied CPU overlay paths on an opaque YUV420p frame")]
struct Args {
    #[arg(long)]
    video: Option<PathBuf>,
    #[arg(long)]
    graphic: Option<PathBuf>,
    #[arg(long, default_value_t = 3840)]
    width: usize,
    #[arg(long, default_value_t = 2160)]
    height: usize,
    #[arg(long, value_enum, default_value_t = Matrix::Bt709)]
    matrix: Matrix,
    #[arg(long, value_enum, default_value_t = Range::Limited)]
    range: Range,
    #[arg(long, value_enum, default_value_t = Transfer::Bt709)]
    video_transfer: Transfer,
    /// PNG normally stores straight sRGB; use only for a nonstandard premultiplied file.
    #[arg(long)]
    input_premultiplied: bool,
    #[arg(long, default_value_t = 1.0)]
    opacity: f64,
    #[arg(long, default_value_t = 9)]
    samples: usize,
    #[arg(long, default_value_t = 4)]
    threads: usize,
    /// Select one or more paths, otherwise run all (see --help).
    #[arg(long, value_enum, value_delimiter = ',')]
    methods: Vec<Method>,
    #[arg(long, default_value = "tmp/frame-blend")]
    output: PathBuf,
    #[arg(long)]
    no_images: bool,
    /// Render black 24/12/48pt cosmic-text overlays and benchmark all four passes.
    #[arg(long)]
    text_demo: bool,
    /// Render the identical text demo with the engine's original YUVA420/YUV blend path.
    #[arg(long, conflicts_with = "text_demo")]
    text_demo_yuv: bool,
    /// Compare serial YUV blending with a fixed Rayon pool (--threads).
    #[arg(long, requires = "text_demo_yuv")]
    compare_yuv_threads: bool,
    /// Additional logo for the production YUV text demo (settings supplied by the caller).
    #[arg(long, requires = "text_demo_yuv")]
    logo: Option<PathBuf>,
    #[arg(long, requires = "logo")]
    logo_scale: Option<String>,
    #[arg(long, requires = "logo")]
    logo_position: Option<String>,
    #[arg(long, requires = "logo")]
    logo_opacity: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum Method {
    RgbaScalar,
    RgbaSimd,
    RgbaSimdRoi,
    RgbaParallelRoi,
    TinySkia,
    LinearPow,
    LinearLutSimd,
    LinearLutRoi,
    LinearLutSparse,
    LinearFusedRoi,
    LinearFusedLutRoi,
    LinearFusedLutParallel,
    LinearFusedLutSimd,
    LinearFusedLutSimdParallel,
    Yuva444Scalar,
    Yuva420Scalar,
    Yuva420Simd,
    Yuva420SimdRoi,
}

impl Method {
    fn all() -> Vec<Self> {
        use Method::*;
        vec![
            RgbaScalar,
            RgbaSimd,
            RgbaSimdRoi,
            RgbaParallelRoi,
            TinySkia,
            LinearPow,
            LinearLutSimd,
            LinearLutRoi,
            LinearLutSparse,
            LinearFusedRoi,
            LinearFusedLutRoi,
            LinearFusedLutParallel,
            LinearFusedLutSimd,
            LinearFusedLutSimdParallel,
            Yuva444Scalar,
            Yuva420Scalar,
            Yuva420Simd,
            Yuva420SimdRoi,
        ]
    }

    fn name(self) -> String {
        self.to_possible_value()
            .expect("all methods have names")
            .get_name()
            .to_owned()
    }

    fn linear(self) -> bool {
        matches!(
            self,
            Self::LinearPow
                | Self::LinearLutSimd
                | Self::LinearLutRoi
                | Self::LinearLutSparse
                | Self::LinearFusedRoi
                | Self::LinearFusedLutRoi
                | Self::LinearFusedLutParallel
                | Self::LinearFusedLutSimd
                | Self::LinearFusedLutSimdParallel
        )
    }

    fn sparse(self) -> bool {
        matches!(
            self,
            Self::RgbaSimdRoi
                | Self::RgbaParallelRoi
                | Self::LinearLutRoi
                | Self::LinearLutSparse
                | Self::LinearFusedRoi
                | Self::LinearFusedLutRoi
                | Self::LinearFusedLutParallel
                | Self::LinearFusedLutSimd
                | Self::LinearFusedLutSimdParallel
                | Self::Yuva420SimdRoi
        )
    }

    fn rgba(self) -> bool {
        matches!(
            self,
            Self::RgbaScalar
                | Self::RgbaSimd
                | Self::RgbaSimdRoi
                | Self::RgbaParallelRoi
                | Self::TinySkia
        )
    }

    fn float(self) -> bool {
        matches!(
            self,
            Self::LinearPow | Self::LinearLutSimd | Self::LinearLutRoi | Self::LinearLutSparse
        )
    }

    fn yuv(self) -> bool {
        !self.rgba() && !self.linear()
    }

    fn rect(self, data: &Data) -> Rect {
        if self.sparse() {
            data.roi
        } else {
            Rect::full(data.width, data.height)
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(
        args.width > 0
            && args.height > 0
            && args.width.is_multiple_of(2)
            && args.height.is_multiple_of(2),
        "positive even frame dimensions required"
    );
    ensure!(
        args.width <= 16384 && args.height <= 16384,
        "frame dimensions exceed this example's limit"
    );
    ensure!(
        args.samples >= 3 && args.threads > 0,
        "at least 3 samples and 1 thread required"
    );
    ensure!(
        args.opacity > 0.0 && args.opacity <= 1.0,
        "opacity must be in (0, 1]"
    );
    let assets =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage/media_images");
    let video = args
        .video
        .clone()
        .unwrap_or_else(|| assets.join("pexels-maxi-7339377.png"));
    let graphic = args
        .graphic
        .clone()
        .unwrap_or_else(|| assets.join("lower-third.png"));
    let started = Instant::now();
    let data = Data::load(
        &video,
        &graphic,
        args.width,
        args.height,
        Color::new(args.matrix, args.range, args.video_transfer),
        args.opacity,
        args.input_premultiplied,
    )?;
    let preparation_ms = started.elapsed().as_secs_f64() * 1000.0;
    let pool = ThreadPoolBuilder::new().num_threads(args.threads).build()?;
    if args.text_demo || args.text_demo_yuv {
        return text_demo::run(&args, &data, &pool, &graphic);
    }
    let methods = if args.methods.is_empty() {
        Method::all()
    } else {
        args.methods.clone()
    };
    let mut scratch = Scratch::new(&data);
    fs::create_dir_all(&args.output)?;
    println!(
        "{}x{} YUV420p {:?}/{:?}/{:?}; sRGB PNG, opacity={}",
        args.width, args.height, args.matrix, args.range, args.video_transfer, args.opacity
    );
    println!(
        "SIMD: {}; Rayon threads={}",
        kernels::backend(),
        pool.current_num_threads()
    );
    println!(
        "alpha ROI: {:?}; static preparation={preparation_ms:.1} ms; retained source/cache={:.1} MiB; scratch={:.1} MiB",
        data.roi,
        data.static_bytes() as f64 / 1048576.0,
        scratch.bytes() as f64 / 1048576.0
    );
    println!("Preparing independent f64 nonlinear and linear references ...");
    let encoded_reference = data.reference(false);
    let linear_reference = data.reference(true);
    let yuv_reference = data.reference_yuv();
    let mut pipeline_times = vec![Vec::new(); methods.len()];
    let mut kernel_times = vec![Vec::new(); methods.len()];

    // Warm every path, then rotate sample order to reduce heat/order bias.
    for method in &methods {
        render(*method, &data, &mut scratch, &pool)?;
    }

    for pass in 0..args.samples {
        for offset in 0..methods.len() {
            let index = (offset + pass) % methods.len();
            let method = methods[index];
            let start = Instant::now();
            render(method, black_box(&data), black_box(&mut scratch), &pool)?;
            pipeline_times[index].push(start.elapsed().as_secs_f64() * 1000.0);
            black_box(&scratch.output);

            // Reset OUTSIDE the kernel timer. Every sample sees a fresh frame;
            // repeated blending into the same frame would benchmark saturation.
            prepare(method, &data, &mut scratch)?;
            let start = Instant::now();
            blend(method, black_box(&data), black_box(&mut scratch), &pool);
            kernel_times[index].push(start.elapsed().as_secs_f64() * 1000.0);
            black_box(&scratch);
        }
        println!("sample {}/{} complete", pass + 1, args.samples);
    }

    let mut report = format!(
        "# 4K premultiplied overlay comparison\n\nFrame: {}x{}, {:?}, {:?}, video transfer {:?}; graphic sRGB, opacity {}.\n\nSIMD: `{}`; fixed Rayon pool: {} threads; {} samples per method.\n\nStatic preparation: {preparation_ms:.2} ms. Retained inputs/prepared overlays: {:.1} MiB; shared reusable scratch: {:.1} MiB (not RSS).\n\n",
        args.width,
        args.height,
        args.matrix,
        args.range,
        args.video_transfer,
        args.opacity,
        kernels::backend(),
        args.threads,
        args.samples,
        data.static_bytes() as f64 / 1048576.0,
        scratch.bytes() as f64 / 1048576.0
    );
    let header = "| Path | Kernel median ms | Pipeline median ms | Pipeline p95 ms | fps | Domain PSNR dB | Max error | Linear-reference PSNR dB |\n|---|---:|---:|---:|---:|---:|---:|---:|\n";
    report.push_str(header);
    let mut csv = String::from(
        "method,domain,kernel_median_ms,pipeline_median_ms,pipeline_p95_ms,fps,domain_psnr,domain_max_error,linear_reference_psnr\n",
    );

    for (index, method) in methods.iter().copied().enumerate() {
        render(method, &data, &mut scratch, &pool)?;
        let reference = if method.yuv() {
            &yuv_reference
        } else if method.linear() {
            &linear_reference
        } else {
            &encoded_reference
        };
        let (psnr, max) = quality(&scratch.output, reference, &data);
        let (linear_psnr, _) = quality(&scratch.output, &linear_reference, &data);
        let median = percentile(&mut pipeline_times[index], 0.5);
        let p95 = percentile(&mut pipeline_times[index], 0.95);
        let kernel = percentile(&mut kernel_times[index], 0.5);
        let row = format!(
            "| {} | {kernel:.3} | {median:.3} | {p95:.3} | {:.1} | {psnr:.2} | {max} | {linear_psnr:.2} |\n",
            method.name(),
            1000.0 / median
        );
        print!("{row}");
        report.push_str(&row);
        csv.push_str(&format!(
            "{},{},{kernel:.6},{median:.6},{p95:.6},{:.3},{psnr:.6},{max},{linear_psnr:.6}\n",
            method.name(),
            if method.linear() {
                "linear"
            } else {
                "nonlinear"
            },
            1000.0 / median
        ));

        if !args.no_images {
            fs::write(
                args.output.join(format!("{}.yuv", method.name())),
                &scratch.output,
            )?;
            save_png(
                &args.output.join(format!("{}.png", method.name())),
                &scratch.output,
                &data,
            )?;
        }
    }

    if !args.no_images {
        save_png(
            &args.output.join("reference-linear.png"),
            &linear_reference,
            &data,
        )?;
        save_png(
            &args.output.join("reference-nonlinear.png"),
            &encoded_reference,
            &data,
        )?;
        save_png(
            &args.output.join("reference-yuv.png"),
            &yuv_reference,
            &data,
        )?;
    }
    report.push_str("\nPipeline includes per-frame conversion, blend, output quantization and necessary copies into YUV420p. PNG decoding, static premultiplication, thread-pool creation, references, metrics and file I/O are excluded. Kernel-only numbers exclude conversion/copy except the fused ROI path, whose kernel IS conversion+blend. Buffers are reused; static overlay preparation assumes an unchanged graphic.\n\nPSNR/max error are unweighted Y/U/V code-value errors over the even-aligned overlay rectangle only (including chroma). 'Domain' compares against f64 composition in the method's own domain: clipped RGB for RGB paths, native (unclipped) YUV for YUVA paths. The linear-reference column includes intended visual differences of nonlinear blending and gamut clipping, not just numerical error. Infinite PSNR means byte-identical output. All outputs are 8-bit YUV420p; this is not an HDR or end-to-end encoder benchmark. See docs/frame-blend.md for limitations and crate research.\n");
    fs::write(args.output.join("report.md"), report)?;
    fs::write(args.output.join("results.csv"), csv)?;
    println!("Results: {}", args.output.display());

    Ok(())
}

fn render(method: Method, data: &Data, scratch: &mut Scratch, pool: &ThreadPool) -> Result<()> {
    prepare(method, data, scratch)?;
    blend(method, data, scratch, pool);
    finish(method, data, scratch)
}

fn percentile(samples: &mut [f64], fraction: f64) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[((samples.len() as f64 * fraction).ceil() as usize).saturating_sub(1)]
}

fn quality(actual: &[u8], expected: &[u8], data: &Data) -> (f64, u8) {
    let pixels = data.width * data.height;
    let mut square = 0_u64;
    let mut count = 0_u64;
    let mut maximum = 0;

    for (offset, width, rect) in [
        (0, data.width, data.roi),
        (pixels, data.width / 2, data.roi.chroma()),
        (pixels + pixels / 4, data.width / 2, data.roi.chroma()),
    ] {
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                let i = offset + y * width + x;
                let error = actual[i].abs_diff(expected[i]);
                square += u64::from(error).pow(2);
                maximum = maximum.max(error);
                count += 1;
            }
        }
    }

    (
        if square == 0 {
            f64::INFINITY
        } else {
            10.0 * (255.0_f64.powi(2) * count as f64 / square as f64).log10()
        },
        maximum,
    )
}

fn save_png(path: &std::path::Path, yuv: &[u8], data: &Data) -> Result<()> {
    let mut rgb = vec![0; data.width * data.height * 3];
    for y in 0..data.height {
        for x in 0..data.width {
            let values = source_rgb(yuv, data.width, data.height, x, y, data.color);
            let start = (y * data.width + x) * 3;
            for c in 0..3 {
                rgb[start + c] =
                    quantize(Transfer::Srgb.encode(data.color.transfer.decode(values[c])) * 255.0);
            }
        }
    }
    PngEncoder::new_with_quality(
        fs::File::create(path)?,
        CompressionType::Fast,
        FilterType::Adaptive,
    )
    .write_image(
        &rgb,
        data.width as u32,
        data.height as u32,
        ExtendedColorType::Rgb8,
    )
    .with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rten_simd::{SimdOp, isa::GenericIsa};

    #[test]
    fn high_precision_simd_covers_gamut_extremes_tails_and_both_color_models() -> Result<()> {
        let directory =
            std::env::temp_dir().join(format!("ffplayout-linear-simd-{}", std::process::id()));
        fs::create_dir_all(&directory)?;
        let video = directory.join("frame.yuv");
        let graphic = directory.join("overlay.png");
        let (width, height) = (70, 6);
        let raw: Vec<u8> = (0..width * height * 3 / 2)
            .map(|i| (i * 137 + i / 7) as u8)
            .collect();
        fs::write(&video, &raw)?;
        let image = image::RgbaImage::from_fn(width as u32, height as u32, |x, y| {
            let index = x + y * width as u32;
            image::Rgba([
                (index * 71) as u8,
                (index * 19) as u8,
                (index * 103) as u8,
                match index % 5 {
                    0 => 0,
                    1 => 1,
                    2 => 128,
                    _ => 255,
                },
            ])
        });
        image.save(&graphic)?;
        let pool = ThreadPoolBuilder::new().num_threads(3).build()?;

        for matrix in [Matrix::Bt709, Matrix::Bt601] {
            for range in [Range::Limited, Range::Full] {
                for transfer in [Transfer::Bt709, Transfer::Srgb] {
                    let mut data = Data::load(
                        &video,
                        &graphic,
                        width,
                        height,
                        Color::new(matrix, range, transfer),
                        0.7,
                        false,
                    )?;
                    let reference = data.reference(true);
                    let mut scratch = Scratch::new(&data);
                    render(Method::LinearFusedLutSimd, &data, &mut scratch, &pool)?;
                    let (_, max) = quality(&scratch.output, &reference, &data);
                    assert!(
                        max <= 1,
                        "{matrix:?}/{range:?}/{transfer:?}: maximum error {max}"
                    );
                    let sequential = scratch.output.clone();
                    render(
                        Method::LinearFusedLutSimdParallel,
                        &data,
                        &mut scratch,
                        &pool,
                    )?;
                    assert_eq!(scratch.output, sequential, "parallel changed pixels");
                    let mut generic = data.video.clone();
                    let pixels = width * height;
                    let (y, uv) = generic.split_at_mut(pixels);
                    let (u, v) = uv.split_at_mut(pixels / 4);
                    for (row, ((y, u), v)) in y
                        .chunks_mut(width * 2)
                        .zip(u.chunks_mut(width / 2))
                        .zip(v.chunks_mut(width / 2))
                        .enumerate()
                        .skip(data.roi.y / 2)
                        .take(data.roi.h / 2)
                    {
                        linear_simd::Rows {
                            data: (&data).into(),
                            video: Some(&data.video),
                            row: row * 2,
                            y,
                            u,
                            v,
                        }
                        .eval(GenericIsa::new());
                    }
                    assert_eq!(generic, sequential, "generic and dispatched SIMD disagree");
                    let mut in_place = data.video.clone();
                    linear_simd::blend_in_place((&data).into(), &mut in_place, &pool);
                    assert_eq!(in_place, sequential, "in-place blending changed pixels");
                    // A following overlay must read the previous pass's output,
                    // never the original video frame retained in the fixture.
                    data.video.copy_from_slice(&sequential);
                    render(Method::LinearFusedLutSimd, &data, &mut scratch, &pool)?;
                    linear_simd::blend_in_place((&data).into(), &mut in_place, &pool);
                    assert_eq!(
                        in_place, scratch.output,
                        "following overlay used stale video"
                    );
                }
            }
        }

        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn every_path_handles_transparency_and_half_coverage_without_double_alpha() -> Result<()> {
        let directory =
            std::env::temp_dir().join(format!("ffplayout-frame-blend-{}", std::process::id()));
        fs::create_dir_all(&directory)?;
        let video = directory.join("frame.yuv");
        let graphic = directory.join("overlay.png");
        // Neutral gray avoids gamut clipping. Hidden saturated pixels test
        // alpha-weighted chroma: transparent colors must never leak into UV.
        let mut raw = vec![100; 8 * 4];
        raw.extend([128; 8 * 4 / 2]);
        fs::write(&video, &raw)?;
        let mut image = image::RgbaImage::new(8, 4);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = if x < 2 || y < 2 || (x == 3 && y == 2) {
                image::Rgba([0, 255, 0, 0])
            } else if x < 4 {
                image::Rgba([200, 40, 80, 128])
            } else {
                image::Rgba([255, 255, 255, 255])
            };
        }
        image.save(&graphic)?;
        let pool = ThreadPoolBuilder::new().num_threads(2).build()?;

        for (matrix, range, transfer) in [
            (Matrix::Bt709, Range::Limited, Transfer::Bt709),
            (Matrix::Bt601, Range::Full, Transfer::Srgb),
        ] {
            let data = Data::load(
                &video,
                &graphic,
                8,
                4,
                Color::new(matrix, range, transfer),
                0.7,
                false,
            )?;
            let linear = data.reference(true);
            let encoded = data.reference(false);
            let native_yuv = data.reference_yuv();
            let mut scratch = Scratch::new(&data);

            for method in Method::all() {
                render(method, &data, &mut scratch, &pool)?;
                let reference = if method.yuv() {
                    &native_yuv
                } else if method.linear() {
                    &linear
                } else {
                    &encoded
                };
                let (_, max) = quality(&scratch.output, reference, &data);
                assert!(max <= 3, "{method:?} {matrix:?} {range:?} error={max}");
                assert!(
                    scratch.output[..16].iter().all(|v| v.abs_diff(100) <= 1),
                    "transparent pixels modified by {method:?}"
                );
            }
        }

        fs::remove_dir_all(directory)?;
        Ok(())
    }
}
