//! Whole YUV420 overlays: production integer kernel versus experimental linear light.
use std::{hint::black_box, path::Path, time::Instant};

use anyhow::{Result, ensure};
use ffmpeg_next::{frame, util::format::pixel::Pixel};
use image::RgbaImage;
use rayon::ThreadPoolBuilder;

use super::{
    color::{Color, Matrix, Range, Rect, Transfer, quantize},
    linear_simd::{self, Overlay},
    overlay::{OverlayFrame, alpha_bounds as frame_alpha_bounds, blend_overlay, chroma_alpha},
    video_input::load_video,
};

const WIDTH: usize = 3840;
const HEIGHT: usize = 2160;
const SAMPLES: usize = 101;

pub fn run() -> Result<()> {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests_assets/storage/media_images");
    let color = Color::new(Matrix::Bt709, Range::Limited, Transfer::Bt709);
    let video = load_video(&root.join("pexels-maxi-7339377.png"), WIDTH, HEIGHT, color)?;
    let decode: Vec<_> = (0..=65535)
        .map(|i| color.transfer.decode(f64::from(i) / 65535.0) as f32)
        .collect();
    let encode: Vec<_> = (0..=65535)
        .map(|i| color.transfer.encode(f64::from(i) / 65535.0) as f32)
        .collect();
    let one = ThreadPoolBuilder::new().num_threads(1).build()?;
    let four = ThreadPoolBuilder::new().num_threads(4).build()?;
    let graphic = image::open(root.join("lower-third.png"))?.into_rgba8();
    let bounds = alpha_bounds(&graphic);
    let lower = image::imageops::crop_imm(
        &graphic,
        bounds.x as u32,
        bounds.y as u32,
        bounds.w as u32,
        bounds.h as u32,
    )
    .to_image();
    let scenarios = [
        ("synthetic_logo_50", synthetic(50, 50, false)),
        ("synthetic_logo_320", synthetic(320, 180, false)),
        ("synthetic_sparse_text", synthetic(1280, 96, true)),
        ("real_lower_third", lower),
        ("synthetic_hd_overlay", synthetic(1920, 1080, false)),
    ];
    println!("\nComplete overlays on identical 4K source frames; BT.709 limited; opacity=1");
    println!("101 samples, interleaved backends; reset/preparation/allocation excluded");
    println!("Production YUV and linear-light outputs intentionally differ in color semantics.");
    println!(
        "{:<24} {:<16} {:>10} {:>10}",
        "scene", "backend", "median ms", "p95 ms"
    );

    for (name, rgba) in scenarios {
        let width = rgba.width() as usize;
        let height = rgba.height() as usize;
        let roi = Rect {
            x: 120,
            y: 100,
            w: width,
            h: height,
        };
        let prepared = prepare_yuva(&rgba, color);
        let mut production = frame::Video::new(Pixel::YUV420P, WIDTH as u32, HEIGHT as u32);
        let mut destination = video.clone();
        let pixels = width * height;
        let mut planar = vec![0.0; pixels * 4];

        for (i, pixel) in rgba.pixels().enumerate() {
            let alpha = f32::from(pixel[3]) / 255.0;
            for channel in 0..3 {
                planar[channel * pixels + i] =
                    Transfer::Srgb.decode(f64::from(pixel[channel]) / 255.0) as f32 * alpha;
            }
            planar[3 * pixels + i] = 1.0 - alpha;
        }

        let view = Overlay {
            width: WIDTH,
            height: HEIGHT,
            roi,
            color,
            linear_planar: &planar,
            decode_fine: &decode,
            encode_fine: &encode,
        };
        blend_direct(view, &mut destination);
        let expected = destination.clone();
        destination.copy_from_slice(&video);
        linear_simd::blend_in_place(view, &mut destination, &one);
        ensure!(
            destination == expected,
            "direct/single-worker results differ: {name}"
        );
        destination.copy_from_slice(&video);
        linear_simd::blend_in_place(view, &mut destination, &four);
        ensure!(
            destination == expected,
            "single/four-thread results differ: {name}"
        );
        drop(expected);
        let mut timings: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::new());
        for sample in 0..SAMPLES + 5 {
            // Rotate order to reduce systematic clock/thermal/cache bias.
            for index in 0..4 {
                let backend = (sample + index) % 4;
                if backend == 0 {
                    reset_frame(&mut production, &video);
                } else {
                    destination.copy_from_slice(&video);
                }
                let start = Instant::now();
                match backend {
                    0 => blend_overlay(black_box(&mut production), prepared.as_ref(), 1.0),
                    1 => blend_direct(view, black_box(&mut destination)),
                    2 => linear_simd::blend_in_place(view, black_box(&mut destination), &one),
                    _ => linear_simd::blend_in_place(view, black_box(&mut destination), &four),
                }
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                black_box(&production);
                black_box(&destination);
                if sample >= 5 {
                    timings[backend].push(elapsed);
                }
            }
        }
        for (backend, values) in [
            "production_yuv",
            "linear_direct",
            "linear_pool_1",
            "linear_pool_4",
        ]
        .into_iter()
        .zip(&mut timings)
        {
            values.sort_by(f64::total_cmp);
            println!(
                "{name:<24} {backend:<16} {:>10.4} {:>10.4}",
                values[SAMPLES / 2],
                values[(SAMPLES - 1) * 95 / 100]
            );
        }
    }
    Ok(())
}

/// The same SIMD row kernel, called synchronously on the benchmark thread.
fn blend_direct(data: Overlay<'_>, output: &mut [u8]) {
    let pixels = data.width * data.height;
    let (y, uv) = output.split_at_mut(pixels);
    let (u, v) = uv.split_at_mut(pixels / 4);
    let first = data.roi.y;
    let end = first + data.roi.h;
    let chroma_first = first / 2 * (data.width / 2);
    let chroma_end = end / 2 * (data.width / 2);

    for (row, ((y, u), v)) in y[first * data.width..end * data.width]
        .chunks_mut(data.width * 2)
        .zip(u[chroma_first..chroma_end].chunks_mut(data.width / 2))
        .zip(v[chroma_first..chroma_end].chunks_mut(data.width / 2))
        .enumerate()
    {
        linear_simd::Rows {
            data,
            video: None,
            row: first + row * 2,
            y,
            u,
            v,
        }
        .run();
    }
}

fn reset_frame(frame: &mut frame::Video, video: &[u8]) {
    let mut offset = 0;
    for (plane, (width, height)) in [
        (WIDTH, HEIGHT),
        (WIDTH / 2, HEIGHT / 2),
        (WIDTH / 2, HEIGHT / 2),
    ]
    .into_iter()
    .enumerate()
    {
        let stride = frame.stride(plane);
        for row in 0..height {
            frame.data_mut(plane)[row * stride..row * stride + width]
                .copy_from_slice(&video[offset..offset + width]);
            offset += width;
        }
    }
}

fn prepare_yuva(rgba: &RgbaImage, color: Color) -> OverlayFrame {
    let (width, height) = rgba.dimensions();
    let mut frame = frame::Video::new(Pixel::YUVA420P, width, height);
    for y in 0..height as usize {
        for x in 0..width as usize {
            let p = rgba.get_pixel(x as u32, y as u32);
            let rgb = std::array::from_fn(|c| {
                color
                    .transfer
                    .encode(Transfer::Srgb.decode(f64::from(p[c]) / 255.0))
            });
            let yy = color.yuv(rgb)[0];
            let ys = frame.stride(0);
            let alpha_stride = frame.stride(3);
            frame.data_mut(0)[y * ys + x] = quantize(yy);
            frame.data_mut(3)[y * alpha_stride + x] = p[3];
        }
    }
    for y in 0..height as usize / 2 {
        for x in 0..width as usize / 2 {
            let mut rgb = [0.0; 3];
            for dy in 0..2 {
                for dx in 0..2 {
                    let p = rgba.get_pixel((x * 2 + dx) as u32, (y * 2 + dy) as u32);
                    for c in 0..3 {
                        rgb[c] += color
                            .transfer
                            .encode(Transfer::Srgb.decode(f64::from(p[c]) / 255.0))
                            / 4.0;
                    }
                }
            }
            let yuv = color.yuv(rgb);
            for (plane, value) in [(1, yuv[1]), (2, yuv[2])] {
                let stride = frame.stride(plane);
                frame.data_mut(plane)[y * stride + x] = quantize(value);
            }
        }
    }
    OverlayFrame {
        lease: None,
        bounds: frame_alpha_bounds(&frame),
        chroma_alpha: chroma_alpha(&frame, width, height),
        frame,
        x: 120,
        y: 100,
        width,
        height,
        opacity: 255,
    }
}

fn synthetic(width: u32, height: u32, sparse: bool) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let alpha = if sparse && (x % 32 > 7 || y % 24 > 17) {
            0
        } else {
            179
        };
        image::Rgba([(x % 256) as u8, (y % 256) as u8, 170, alpha])
    })
}

fn alpha_bounds(image: &RgbaImage) -> Rect {
    let (mut left, mut top, mut right, mut bottom) = (image.width(), image.height(), 0, 0);
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] != 0 {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x + 1);
            bottom = bottom.max(y + 1);
        }
    }
    let x = left as usize & !1;
    let y = top as usize & !1;
    Rect {
        x,
        y,
        w: ((right as usize + 1) & !1) - x,
        h: ((bottom as usize + 1) & !1) - y,
    }
}
