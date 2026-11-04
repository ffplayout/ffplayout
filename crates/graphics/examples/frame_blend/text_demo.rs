//! Engine-like text example: cosmic-text + Swash (same stack as
//! src/rendering/text/mod.rs), then independent cached linear SIMD overlays.
use std::{fs, hint::black_box, path::Path, time::Instant};

use anyhow::{Result, ensure};
use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap,
};
use image::RgbaImage;
use rayon::ThreadPool;

use super::{
    Args,
    color::Rect,
    cpu_time::process_cpu_seconds,
    data::Data,
    linear_simd::{self, Overlay},
    percentile, save_png,
};

struct Sentence<'a> {
    text: &'a str,
    bold: &'a str,
    italic: &'a str,
    points: f32,
    top: usize,
}

struct TextOverlay {
    roi: Rect,
    planar: Vec<f32>,
}

impl TextOverlay {
    fn view<'a>(&'a self, data: &'a Data) -> Overlay<'a> {
        Overlay {
            roi: self.roi,
            linear_planar: &self.planar,
            ..data.into()
        }
    }
}

fn rasterize(
    sentence: &Sentence<'_>,
    data: &Data,
    fonts: &mut FontSystem,
    cache: &mut SwashCache,
) -> Result<(RgbaImage, TextOverlay)> {
    let scale = layout_scale(data);
    let left = ((120.0 * scale).round() as usize) & !1;
    let top = ((sentence.top as f32 * scale).round() as usize) & !1;
    let font_px = sentence.points * 96.0 / 72.0 * scale;
    let width = data.width - left * 2;
    let height = (font_px * 1.5).ceil() as usize;
    ensure!(top + height < data.height, "text does not fit this frame");
    let base = Attrs::new().family(Family::Name("Arial"));
    let mut segments = [
        (
            sentence.text.find(sentence.bold).unwrap(),
            sentence.bold.len(),
            true,
        ),
        (
            sentence.text.find(sentence.italic).unwrap(),
            sentence.italic.len(),
            false,
        ),
    ];
    segments.sort_by_key(|segment| segment.0);
    let mut spans = Vec::new();
    let mut offset = 0;
    for (start, length, bold) in segments {
        spans.push((&sentence.text[offset..start], base.clone()));
        spans.push((
            &sentence.text[start..start + length],
            if bold {
                base.clone().weight(Weight::BOLD)
            } else {
                base.clone().style(Style::Italic)
            },
        ));
        offset = start + length;
    }
    spans.push((&sentence.text[offset..], base.clone()));
    let mut buffer = Buffer::new(fonts, Metrics::new(font_px, font_px * 1.25));
    buffer.set_size(Some(width as f32), Some(height as f32));
    buffer.set_wrap(Wrap::None);
    buffer.set_rich_text(spans, &base, Shaping::Advanced, None);
    buffer.shape_until_scroll(fonts, false);
    ensure!(
        buffer.layout_runs().all(|line| line.line_w <= width as f32),
        "text is wider than the available frame"
    );
    let mut bitmap = RgbaImage::new(width as u32, height as u32);
    let mut min = [width, height];
    let mut max = [0, 0];
    buffer.draw(fonts, cache, Color::rgb(0, 0, 0), |x, y, w, h, color| {
        for dy in 0..h {
            for dx in 0..w {
                let px = x + dx as i32;
                let py = y + dy as i32;
                if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                    continue;
                }
                let (px, py) = (px as usize, py as usize);
                let pixel = bitmap.get_pixel_mut(px as u32, py as u32);
                let alpha = u16::from(color.a());
                pixel[3] = (alpha + (u16::from(pixel[3]) * (255 - alpha) + 127) / 255) as u8;
                if pixel[3] > 0 {
                    min[0] = min[0].min(px);
                    min[1] = min[1].min(py);
                    max[0] = max[0].max(px + 1);
                    max[1] = max[1].max(py + 1);
                }
            }
        }
    });
    ensure!(
        max[0] > min[0] && max[1] > min[1],
        "font produced no visible text"
    );
    let x = (left + min[0]) & !1;
    let y = (top + min[1]) & !1;
    let right = (left + max[0] + 1) & !1;
    let bottom = (top + max[1] + 1) & !1;
    let roi = Rect {
        x,
        y,
        w: right - x,
        h: bottom - y,
    };
    let area = roi.w * roi.h;
    // Black remains zero in both sRGB and linear RGB. Only inverse coverage
    // must be populated. No extra 8-bit YUVA intermediate is introduced.
    let mut planar = vec![0.0; area * 4];
    for py in 0..roi.h {
        for px in 0..roi.w {
            let sx = roi.x + px - left;
            let sy = roi.y + py - top;
            let alpha = if sx < width && sy < height {
                bitmap.get_pixel(sx as u32, sy as u32)[3]
            } else {
                0
            };
            planar[3 * area + py * roi.w + px] = 1.0 - f32::from(alpha) / 255.0;
        }
    }

    Ok((bitmap, TextOverlay { roi, planar }))
}

pub fn run(args: &Args, data: &Data, pool: &ThreadPool, graphic: &Path) -> Result<()> {
    ensure!(
        data.width >= 1280 && data.height >= 720,
        "text demo requires at least 1280x720"
    );
    fs::create_dir_all(&args.output)?;
    let sentences = [
        Sentence {
            text: "Dieses Bild zeigt präzises Compositing mit linearem Licht und sauberer Transparenz.",
            bold: "präzises Compositing",
            italic: "linearem Licht",
            points: 24.0,
            top: 96,
        },
        Sentence {
            text: "Auch kleine Schrift bleibt mit fetten und kursiven Wörtern gut lesbar.",
            bold: "kleine Schrift",
            italic: "kursiven Wörtern",
            points: 12.0,
            top: 164,
        },
        Sentence {
            text: "Mit großer Schrift bleiben Details auch in 4K klar erkennbar.",
            bold: "großer Schrift",
            italic: "klar erkennbar",
            points: 48.0,
            top: 216,
        },
    ];
    let raster_start = Instant::now();
    let mut fonts = FontSystem::new();
    let mut cache = SwashCache::new();
    let mut text = Vec::new();
    for sentence in &sentences {
        let (bitmap, overlay) = rasterize(sentence, data, &mut fonts, &mut cache)?;
        if !args.no_images {
            bitmap.save(args.output.join(format!("text-{}pt.png", sentence.points)))?;
        }
        text.push(overlay);
    }
    let raster_ms = raster_start.elapsed().as_secs_f64() * 1000.0;
    if args.text_demo_yuv {
        return run_yuv(args, data, graphic, &text, pool);
    }
    let overlays: Vec<_> = std::iter::once(data.into())
        .chain(text.iter().map(|text| text.view(data)))
        .collect();
    let mut output = data.video.clone();
    for _ in 0..3 {
        output.copy_from_slice(&data.video);
        for overlay in &overlays {
            linear_simd::blend_in_place(*overlay, &mut output, pool);
        }
    }
    let mut total = Vec::new();
    let mut copy_blend = Vec::new();
    let mut passes = vec![Vec::new(); overlays.len()];
    for _ in 0..args.samples {
        let full_start = Instant::now();
        output.copy_from_slice(&data.video);
        let blend_start = Instant::now();
        for overlay in &overlays {
            linear_simd::blend_in_place(*overlay, black_box(&mut output), pool);
        }
        total.push(blend_start.elapsed().as_secs_f64() * 1000.0);
        copy_blend.push(full_start.elapsed().as_secs_f64() * 1000.0);
        black_box(&output);

        output.copy_from_slice(&data.video);
        for (index, overlay) in overlays.iter().enumerate() {
            let start = Instant::now();
            linear_simd::blend_in_place(*overlay, black_box(&mut output), pool);
            passes[index].push(start.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let median = percentile(&mut total, 0.5);
    let p95 = percentile(&mut total, 0.95);
    let copy_median = percentile(&mut copy_blend, 0.5);
    let mut report = format!(
        "# Engine-like text and lower-third overlay\n\n{}x{}, {:?}/{:?}/{:?}, {} threads, {} samples.\n\nText uses cosmic-text/Swash, the same shaping/rasterization stack as the engine, with black glyphs, bold and italic spans. The reference canvas is 3840x2160, with 24/12/48 pt at 96 DPI (32/16/64 pixels); smaller canvases scale text proportionally. Each sentence is cached as an independent tight, even-aligned linear-premultiplied overlay.\n\nTotal blend median: **{median:.3} ms**; p95: {p95:.3} ms.\nFrame reset/copy plus blending median: {copy_median:.3} ms.\n\nThe total timer includes all four sequential overlay passes, their SIMD color transforms/LUTs and thread-pool work. It excludes the initial frame copy, rasterization, preparation, allocation and image I/O. This is wall time, not summed CPU time. Text is cached; changing text requires rasterization again.\n\n| Overlay | Median ms | ROI |\n|---|---:|---|\n",
        data.width,
        data.height,
        args.matrix,
        args.range,
        args.video_transfer,
        args.threads,
        args.samples
    );
    for (index, overlay) in overlays.iter().enumerate() {
        let name = if index == 0 {
            "Lower third".to_owned()
        } else {
            format!("Text {} pt", sentences[index - 1].points)
        };
        report.push_str(&format!(
            "| {name} | {:.3} | {:?} |\n",
            percentile(&mut passes[index], 0.5),
            overlay.roi
        ));
    }
    report.push_str(&format!("\nOne-time font setup/rasterization/preparation (including diagnostic PNG writing when enabled): {raster_ms:.2} ms (outside timers).\n"));
    fs::write(args.output.join("text-blend-report.md"), &report)?;
    if !args.no_images {
        fs::write(args.output.join("composite.yuv"), &output)?;
        save_png(&args.output.join("composite.png"), &output, data)?;
    }
    println!("{report}");

    Ok(())
}

fn layout_scale(data: &Data) -> f32 {
    (data.width as f32 / 3840.0).min(data.height as f32 / 2160.0)
}

/// Use the production logo decoder and compositor; reuse exactly the linear
/// demo's black glyph coverage, positions and rich-text shaping for comparison.
fn run_yuv(
    args: &Args,
    data: &Data,
    graphic: &Path,
    text: &[TextOverlay],
    pool: &ThreadPool,
) -> Result<()> {
    use ff_graphics::{
        ffmpeg::{
            logo::LogoOverlay,
            overlay::{
                BlendExecution, OverlayBounds, OverlayFrame, alpha_bounds,
                blend_overlay_with_execution, chroma_alpha,
            },
        },
        style::LogoStyle,
    };
    use ffmpeg_next::{format, frame, util::format::pixel::Pixel};

    ensure!(
        !args.input_premultiplied,
        "legacy logo decoder expects a standard straight-alpha image"
    );
    ffmpeg_next::init()?;
    let style = LogoStyle {
        scale: None,
        position: "0:0".into(),
        opacity: args.opacity,
    };
    let input = format::input(graphic)?;
    let logo = LogoOverlay::from_input(
        &style,
        input,
        &graphic.display().to_string(),
        data.width as u32,
        data.height as u32,
    )?;
    let additional_logo = args
        .logo
        .as_ref()
        .map(|path| {
            let style = LogoStyle {
                scale: args.logo_scale.clone(),
                position: args.logo_position.clone().unwrap_or_else(|| "0:0".into()),
                opacity: args.logo_opacity.unwrap_or(1.0),
            };

            LogoOverlay::from_input(
                &style,
                format::input(path)?,
                &path.display().to_string(),
                data.width as u32,
                data.height as u32,
            )
        })
        .transpose()?;
    let mut prepared_text = Vec::new();
    for source in text {
        let roi = source.roi;
        let mut frame = frame::Video::new(Pixel::YUVA420P, roi.w as u32, roi.h as u32);
        // Black maps to limited-range Y=16, U=V=128 in the legacy text conversion.
        frame.data_mut(0).fill(16);
        frame.data_mut(1).fill(128);
        frame.data_mut(2).fill(128);
        frame.data_mut(3).fill(0);
        let stride = frame.stride(3);
        for y in 0..roi.h {
            for x in 0..roi.w {
                let inverse = source.planar[3 * roi.w * roi.h + y * roi.w + x];
                frame.data_mut(3)[y * stride + x] = ((1.0 - inverse) * 255.0).round() as u8;
            }
        }
        prepared_text.push(OverlayFrame {
            lease: None,
            bounds: alpha_bounds(&frame),
            chroma_alpha: chroma_alpha(&frame, roi.w as u32, roi.h as u32),
            frame,
            x: roi.x as i32,
            y: roi.y as i32,
            width: roi.w as u32,
            height: roi.h as u32,
            opacity: 255,
        });
    }
    let mut layers = vec![logo.as_overlay()];
    layers.extend(prepared_text.iter().map(OverlayFrame::as_ref));
    if let Some(logo) = &additional_logo {
        layers.push(logo.as_overlay());
    }
    let mut frame = frame::Video::new(Pixel::YUV420P, data.width as u32, data.height as u32);
    let mut timings: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::new());
    let mut cpu_times: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::new());
    let variants = if args.compare_yuv_threads { 5 } else { 1 };
    let mut reference = Vec::new();
    for sample in 0..args.samples + 3 {
        for index in 0..variants {
            // Interleave and rotate the backends; every pass starts with the same video.
            let variant = (sample + index) % variants;
            let mut offset = 0;
            for (plane, (w, h)) in [
                (data.width, data.height),
                (data.width / 2, data.height / 2),
                (data.width / 2, data.height / 2),
            ]
            .into_iter()
            .enumerate()
            {
                let stride = frame.stride(plane);
                for row in 0..h {
                    frame.data_mut(plane)[row * stride..row * stride + w]
                        .copy_from_slice(&data.video[offset..offset + w]);
                    offset += w;
                }
            }
            let cpu_start = args
                .compare_yuv_threads
                .then(process_cpu_seconds)
                .transpose()?;
            let start = Instant::now();
            for source in &layers {
                let mut overlay = *source;
                if args.compare_yuv_threads && variant < 2 {
                    overlay.bounds = OverlayBounds::full(overlay.width, overlay.height);
                }
                let execution = if !args.compare_yuv_threads || variant == 4 {
                    BlendExecution::Adaptive
                } else if variant % 2 == 0 {
                    BlendExecution::Serial
                } else {
                    BlendExecution::Parallel(pool)
                };
                blend_overlay_with_execution(black_box(&mut frame), overlay, 1.0, execution);
            }
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let cpu_elapsed = if let Some(start) = cpu_start {
                (process_cpu_seconds()? - start) * 1000.0
            } else {
                0.0
            };
            if sample >= 3 {
                timings[variant].push(elapsed);
                cpu_times[variant].push(cpu_elapsed);
            }
            if args.compare_yuv_threads && sample == 0 {
                let bytes = pack_video(&frame, data);
                if variant == 0 {
                    reference = bytes;
                } else {
                    ensure!(
                        bytes == reference,
                        "cropped/parallel YUV output differs from uncropped production"
                    );
                }
            }
            black_box(&frame);
        }
    }
    let output = pack_video(&frame, data);
    if !args.no_images {
        fs::write(args.output.join("composite.yuv"), &output)?;
        save_png(&args.output.join("composite.png"), &output, data)?;
    }
    let mut report = format!(
        "# Legacy engine YUV text and lower-third overlay\n\nSame source frame, black glyphs with bold/italic spans and placements as the linear text demo. Text uses a 3840x2160 reference canvas with 24/12/48 pt, proportionally scaled for other canvas sizes. Uses the production FFmpeg logo decoder (its original YUVA420 conversion defaults) and the unchanged signal-domain YUV SIMD compositor. Text uses the identical cached glyph coverage, converted to black YUVA420 with the legacy text conversion values and chroma-alpha averaging. No linear-light blending.\n\nBlend median: {:.3} ms; p95: {:.3} ms; {} samples. Preparation, frame reset and image I/O excluded. Normal rendering uses cached bounds with adaptive execution; comparison row zero is the uncropped serial baseline. Layout always retains the original image extent.\n",
        percentile(&mut timings[0], 0.5),
        percentile(&mut timings[0], 0.95),
        args.samples
    );
    report.push_str(&format!(
        "\nCanvas: {}x{}; text layout scale: {:.6}; rendered text sizes: {:.2}/{:.2}/{:.2} pt.\n",
        data.width,
        data.height,
        layout_scale(data),
        24.0 * layout_scale(data),
        12.0 * layout_scale(data),
        48.0 * layout_scale(data)
    ));
    if args.compare_yuv_threads {
        report.push_str(&format!("\n{} Rayon workers; five backends alternate order. CPU time is process user+system time including all workers (getrusage). Reset, preparation, equality checks, pool creation and file I/O are excluded. Dispatch/waits are included. Visible YUV output verified byte-identical for every backend.\n\n| Backend | Wall median ms | Wall p95 ms | CPU median ms/frame |\n|---|---:|---:|---:|\n", pool.current_num_threads()));
        for (i, name) in [
            "Serial full",
            "Rayon full",
            "Serial cropped",
            "Rayon cropped",
            "Adaptive cropped",
        ]
        .into_iter()
        .enumerate()
        {
            report.push_str(&format!(
                "| {name} | {:.3} | {:.3} | {:.3} |\n",
                percentile(&mut timings[i], 0.5),
                percentile(&mut timings[i], 0.95),
                percentile(&mut cpu_times[i], 0.5)
            ));
        }
        for overlay in &layers {
            report.push_str(&format!(
                "\nLayer {}x{}, nontransparent bounds {:?}.\n",
                overlay.width, overlay.height, overlay.bounds
            ));
        }
    }
    if let Some(logo) = &additional_logo {
        report.push_str(&format!(
            "\nAdditional logo: {}x{} px at ({}, {}), opacity {:.3}. Settings are supplied by the caller; the graphics crate does not read the application database.\n",
            logo.width, logo.height, logo.x, logo.y, args.logo_opacity.unwrap_or(1.0)
        ));
    }
    fs::write(args.output.join("text-blend-report.md"), &report)?;
    println!("{report}");

    Ok(())
}

fn pack_video(frame: &ffmpeg_next::frame::Video, data: &Data) -> Vec<u8> {
    let mut output = Vec::with_capacity(data.video.len());
    for (plane, (w, h)) in [
        (data.width, data.height),
        (data.width / 2, data.height / 2),
        (data.width / 2, data.height / 2),
    ]
    .into_iter()
    .enumerate()
    {
        let stride = frame.stride(plane);
        for row in 0..h {
            output.extend_from_slice(&frame.data(plane)[row * stride..row * stride + w]);
        }
    }

    output
}
