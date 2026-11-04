use anyhow::Result;
use rayon::{ThreadPool, prelude::*};
use tiny_skia::{PixmapMut, PixmapPaint, PixmapRef, Transform};
use yuv::{
    BufferStoreMut, YuvConversionMode, YuvPlanarImage, YuvPlanarImageMut, YuvRange,
    YuvStandardMatrix,
};

use super::{
    Method,
    color::{Matrix, Range, Rect, Transfer, compose_blocks, quantize, source_rgb},
    data::{Data, Weighted},
    kernels, linear_simd,
};

pub struct Scratch {
    pub output: Vec<u8>,
    rgba: Vec<u8>,
    floats: Vec<f32>,
    yuv444: Vec<u8>,
    skia_source: Vec<u8>,
}

impl Scratch {
    pub fn new(data: &Data) -> Self {
        let pixels = data.width * data.height;

        Self {
            output: data.video.clone(),
            rgba: vec![0; pixels * 4],
            floats: vec![0.0; pixels * 4],
            yuv444: vec![0; pixels * 3],
            skia_source: data.rgba.source.iter().map(|v| (v / 255) as u8).collect(),
        }
    }

    pub fn bytes(&self) -> usize {
        self.output.len()
            + self.rgba.len()
            + self.floats.len() * 4
            + self.yuv444.len()
            + self.skia_source.len()
    }
}

fn parameters(data: &Data) -> (YuvRange, YuvStandardMatrix) {
    (
        match data.color.range {
            Range::Limited => YuvRange::Limited,
            Range::Full => YuvRange::Full,
        },
        match data.color.matrix {
            Matrix::Bt709 => YuvStandardMatrix::Bt709,
            Matrix::Bt601 => YuvStandardMatrix::Bt601,
        },
    )
}

fn to_rgba(data: &Data, dst: &mut [u8]) -> Result<()> {
    let pixels = data.width * data.height;
    let (range, matrix) = parameters(data);
    let source = YuvPlanarImage {
        y_plane: &data.video[..pixels],
        y_stride: data.width as u32,
        u_plane: &data.video[pixels..pixels + pixels / 4],
        u_stride: data.width as u32 / 2,
        v_plane: &data.video[pixels + pixels / 4..],
        v_stride: data.width as u32 / 2,
        width: data.width as u32,
        height: data.height as u32,
    };

    yuv::yuv420_to_rgba(&source, dst, data.width as u32 * 4, range, matrix)?;
    Ok(())
}

fn from_rgba(data: &Data, rgba: &[u8], output: &mut [u8]) -> Result<()> {
    let pixels = data.width * data.height;
    let (y, uv) = output.split_at_mut(pixels);
    let (u, v) = uv.split_at_mut(pixels / 4);
    let (range, matrix) = parameters(data);
    let mut target = YuvPlanarImageMut {
        y_plane: BufferStoreMut::Borrowed(y),
        y_stride: data.width as u32,
        u_plane: BufferStoreMut::Borrowed(u),
        u_stride: data.width as u32 / 2,
        v_plane: BufferStoreMut::Borrowed(v),
        v_stride: data.width as u32 / 2,
        width: data.width as u32,
        height: data.height as u32,
    };

    yuv::rgba_to_yuv420(
        &mut target,
        rgba,
        data.width as u32 * 4,
        range,
        matrix,
        YuvConversionMode::Professional,
    )?;
    Ok(())
}

#[inline(never)]
pub fn prepare(method: Method, data: &Data, scratch: &mut Scratch) -> Result<()> {
    scratch.output.copy_from_slice(&data.video);

    if method.rgba() || method.float() {
        // Even sparse RGBA paths include the full SIMD YUV<->RGBA conversion.
        // Only the fused path below truly avoids full-frame RGB intermediates.
        to_rgba(data, &mut scratch.rgba)?;
    }

    if method.float() {
        let rect = if method == Method::LinearLutSparse {
            data.roi
        } else {
            Rect::full(data.width, data.height)
        };
        for y in rect.y..rect.y + rect.h {
            for x in rect.x..rect.x + rect.w {
                let i = (y * data.width + x) * 4;
                for c in 0..3 {
                    let code = scratch.rgba[i + c];
                    scratch.floats[i + c] = if method == Method::LinearPow {
                        decode_f32(data.color.transfer, f32::from(code) / 255.0)
                    } else {
                        data.decode_lut[usize::from(code)]
                    };
                }
                scratch.floats[i + 3] = 1.0;
            }
        }
    }

    if method == Method::Yuva444Scalar {
        let pixels = data.width * data.height;
        scratch.yuv444[..pixels].copy_from_slice(&data.video[..pixels]);

        for plane in 1..3 {
            for y in 0..data.height {
                for x in 0..data.width {
                    scratch.yuv444[plane * pixels + y * data.width + x] = data.video
                        [pixels + (plane - 1) * pixels / 4 + y / 2 * (data.width / 2) + x / 2];
                }
            }
        }
    }

    Ok(())
}

fn weighted_rows(dst: &mut [u8], overlay: &Weighted, width: usize, rect: Rect, simd: bool) {
    for y in rect.y..rect.y + rect.h {
        let start = y * width + rect.x;
        let end = start + rect.w;
        if simd {
            kernels::over_simd(
                &mut dst[start..end],
                &overlay.source[start..end],
                &overlay.alpha[start..end],
            );
        } else {
            kernels::over_scalar(
                &mut dst[start..end],
                &overlay.source[start..end],
                &overlay.alpha[start..end],
            );
        }
    }
}

#[inline(never)]
pub fn blend(method: Method, data: &Data, scratch: &mut Scratch, pool: &ThreadPool) {
    let rect = method.rect(data);
    let pixels = data.width * data.height;

    if method == Method::RgbaParallelRoi {
        pool.install(|| {
            scratch
                .rgba
                .par_chunks_mut(data.width * 4)
                .enumerate()
                .filter(|(y, _)| *y >= rect.y && *y < rect.y + rect.h)
                .for_each(|(y, row)| {
                    let start = (y * data.width + rect.x) * 4;
                    let end = start + rect.w * 4;
                    kernels::over_simd(
                        &mut row[rect.x * 4..(rect.x + rect.w) * 4],
                        &data.rgba.source[start..end],
                        &data.rgba.alpha[start..end],
                    );
                });
        });
    } else if method == Method::TinySkia {
        let mut dst =
            PixmapMut::from_bytes(&mut scratch.rgba, data.width as u32, data.height as u32)
                .expect("validated dimensions");
        let source =
            PixmapRef::from_bytes(&scratch.skia_source, data.width as u32, data.height as u32)
                .expect("prepared premultiplied RGBA");
        dst.draw_pixmap(
            0,
            0,
            source,
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    } else if method.rgba() {
        weighted_rows(
            &mut scratch.rgba,
            &data.rgba,
            data.width * 4,
            Rect {
                x: rect.x * 4,
                w: rect.w * 4,
                ..rect
            },
            method != Method::RgbaScalar,
        );
    } else if method.float() {
        for y in rect.y..rect.y + rect.h {
            let start = (y * data.width + rect.x) * 4;
            let end = start + rect.w * 4;
            if method == Method::LinearPow {
                for ((d, s), inv) in scratch.floats[start..end]
                    .iter_mut()
                    .zip(&data.linear[start..end])
                    .zip(&data.inverse[start..end])
                {
                    *d = *s + *d * inv;
                }
            } else {
                kernels::over_float(
                    &mut scratch.floats[start..end],
                    &data.linear[start..end],
                    &data.inverse[start..end],
                );
            }
        }
    } else if matches!(method, Method::LinearFusedRoi | Method::LinearFusedLutRoi) {
        compose_blocks(
            &mut scratch.output,
            data.width,
            data.height,
            rect,
            data.color,
            |x, y| linear_pixel(data, x, y, method == Method::LinearFusedLutRoi),
        );
    } else if matches!(
        method,
        Method::LinearFusedLutSimd | Method::LinearFusedLutSimdParallel
    ) {
        let (y, uv) = scratch.output.split_at_mut(pixels);
        let (u, v) = uv.split_at_mut(pixels / 4);
        if method == Method::LinearFusedLutSimdParallel {
            pool.install(|| {
                y.par_chunks_mut(data.width * 2)
                    .zip(u.par_chunks_mut(data.width / 2))
                    .zip(v.par_chunks_mut(data.width / 2))
                    .enumerate()
                    .filter(|(row, _)| row * 2 >= rect.y && row * 2 < rect.y + rect.h)
                    .for_each(|(row, ((y, u), v))| {
                        linear_simd::Rows {
                            data: data.into(),
                            video: Some(&data.video),
                            row: row * 2,
                            y,
                            u,
                            v,
                        }
                        .run();
                    });
            });
        } else {
            for (row, ((y, u), v)) in y
                .chunks_mut(data.width * 2)
                .zip(u.chunks_mut(data.width / 2))
                .zip(v.chunks_mut(data.width / 2))
                .enumerate()
                .skip(rect.y / 2)
                .take(rect.h / 2)
            {
                linear_simd::Rows {
                    data: data.into(),
                    video: Some(&data.video),
                    row: row * 2,
                    y,
                    u,
                    v,
                }
                .run();
            }
        }
    } else if method == Method::LinearFusedLutParallel {
        let (y, uv) = scratch.output.split_at_mut(pixels);
        let (u, v) = uv.split_at_mut(pixels / 4);
        pool.install(|| {
            y.par_chunks_mut(data.width * 2)
                .zip(u.par_chunks_mut(data.width / 2))
                .zip(v.par_chunks_mut(data.width / 2))
                .enumerate()
                .filter(|(row, _)| row * 2 >= rect.y && row * 2 < rect.y + rect.h)
                .for_each(|(row, ((y_plane, u), v))| {
                    for x in (rect.x..rect.x + rect.w).step_by(2) {
                        let mut chroma = [0.0; 2];
                        for dy in 0..2 {
                            for dx in 0..2 {
                                let values =
                                    data.color
                                        .yuv(linear_pixel(data, x + dx, row * 2 + dy, true));
                                y_plane[dy * data.width + x + dx] = quantize(values[0]);
                                chroma[0] += values[1];
                                chroma[1] += values[2];
                            }
                        }
                        u[x / 2] = quantize(chroma[0] / 4.0);
                        v[x / 2] = quantize(chroma[1] / 4.0);
                    }
                });
        });
    } else if method == Method::Yuva444Scalar {
        kernels::over_scalar(
            &mut scratch.yuv444,
            &data.yuva444.source,
            &data.yuva444.alpha,
        );
    } else {
        let simd = method != Method::Yuva420Scalar;
        weighted_rows(
            &mut scratch.output[..pixels],
            &data.yuva420,
            data.width,
            rect,
            simd,
        );
        let chroma = rect.chroma();
        for plane in 0..2 {
            let offset = pixels + plane * pixels / 4;
            let end = offset + pixels / 4;
            for y in chroma.y..chroma.y + chroma.h {
                let start = offset + y * (data.width / 2) + chroma.x;
                let stop = start + chroma.w;
                debug_assert!(stop <= end);
                if simd {
                    kernels::over_simd(
                        &mut scratch.output[start..stop],
                        &data.yuva420.source[start..stop],
                        &data.yuva420.alpha[start..stop],
                    );
                } else {
                    kernels::over_scalar(
                        &mut scratch.output[start..stop],
                        &data.yuva420.source[start..stop],
                        &data.yuva420.alpha[start..stop],
                    );
                }
            }
        }
    }
}

#[inline(never)]
pub fn finish(method: Method, data: &Data, scratch: &mut Scratch) -> Result<()> {
    if method.float() {
        let rect = if method == Method::LinearLutSparse {
            data.roi
        } else {
            Rect::full(data.width, data.height)
        };
        for y in rect.y..rect.y + rect.h {
            let start = (y * data.width + rect.x) * 4;
            let end = start + rect.w * 4;
            for (dst, src) in scratch.rgba[start..end]
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(scratch.floats[start..end].as_chunks::<4>().0)
            {
                for c in 0..3 {
                    dst[c] = if method == Method::LinearPow {
                        quantize(f64::from(encode_f32(data.color.transfer, src[c])) * 255.0)
                    } else {
                        data.encode_lookup(src[c])
                    };
                }
                dst[3] = 255;
            }
        }
    }

    if method.rgba() || method.float() {
        from_rgba(data, &scratch.rgba, &mut scratch.output)?;
    } else if method == Method::Yuva444Scalar {
        let pixels = data.width * data.height;
        scratch.output[..pixels].copy_from_slice(&scratch.yuv444[..pixels]);
        for plane in 1..3 {
            for y in 0..data.height / 2 {
                for x in 0..data.width / 2 {
                    let i = plane * pixels + y * 2 * data.width + x * 2;
                    let sum = u16::from(scratch.yuv444[i])
                        + u16::from(scratch.yuv444[i + 1])
                        + u16::from(scratch.yuv444[i + data.width])
                        + u16::from(scratch.yuv444[i + data.width + 1]);
                    scratch.output[pixels + (plane - 1) * pixels / 4 + y * (data.width / 2) + x] =
                        ((sum + 2) / 4) as u8;
                }
            }
        }
    }

    Ok(())
}

#[inline]
fn linear_pixel(data: &Data, x: usize, y: usize, lut: bool) -> [f64; 3] {
    let i = (y * data.width + x) * 4;
    let base = source_rgb(&data.video, data.width, data.height, x, y, data.color);
    if data.graphic[i + 3] == 0 {
        return base;
    }

    std::array::from_fn(|c| {
        let linear = if lut {
            data.decode_fine[(base[c] * 65535.0).round() as usize]
        } else {
            decode_f32(data.color.transfer, base[c] as f32)
        };
        let blended = data.linear[i + c] + linear * data.inverse[i + c];

        f64::from(if lut {
            data.encode_fine[(blended.clamp(0.0, 1.0) * 65535.0).round() as usize]
        } else {
            encode_f32(data.color.transfer, blended)
        })
    })
}

fn decode_f32(transfer: Transfer, value: f32) -> f32 {
    match transfer {
        Transfer::Srgb if value <= 0.04045 => value / 12.92,
        Transfer::Srgb => ((value + 0.055) / 1.055).powf(2.4),
        Transfer::Bt709 if value < 0.081 => value / 4.5,
        Transfer::Bt709 => ((value + 0.099) / 1.099).powf(1.0 / 0.45),
    }
}

fn encode_f32(transfer: Transfer, value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    match transfer {
        Transfer::Srgb if value <= 0.0031308 => 12.92 * value,
        Transfer::Srgb => 1.055 * value.powf(1.0 / 2.4) - 0.055,
        Transfer::Bt709 if value < 0.018 => 4.5 * value,
        Transfer::Bt709 => 1.099 * value.powf(0.45) - 0.099,
    }
}
