//! High-precision association and coverage-weighted chroma before final quantization.
use anyhow::Result;
use ffmpeg_next::{
    ffi, frame,
    util::{color::Range, format::pixel::Pixel},
};
use rayon::{ThreadPool, prelude::*};

use crate::ffmpeg::compositor::metadata::copy_color_properties;

use super::color::ColorConversion;

pub(super) fn sample(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

/// Associate in encoded RGB before filtering. No YUV offsets participate.
pub(super) fn associate(frame: &mut frame::Video) {
    let stride = frame.stride(0);
    let width = frame.width() as usize;

    for row in frame.data_mut(0).chunks_mut(stride) {
        for pixel in row[..width * 8].as_chunks_mut::<8>().0 {
            let alpha = u64::from(sample(&pixel[6..]));

            for channel in pixel[..6].as_chunks_mut::<2>().0 {
                let value = (u64::from(sample(channel)) * alpha + 32767) / 65535;
                channel.copy_from_slice(&(value as u16).to_le_bytes());
            }
        }
    }
}

pub(super) fn attach_alpha(alpha: &frame::Video, rgb: &mut frame::Video) {
    let stride = rgb.stride(0);
    let mask_stride = alpha.stride(0);
    let width = rgb.width() as usize;

    for row in 0..rgb.height() as usize {
        let pixels = &mut rgb.data_mut(0)[row * stride..row * stride + width * 8];
        let mask = &alpha.data(0)[row * mask_stride..row * mask_stride + width * 2];

        for (pixel, coverage) in pixels
            .as_chunks_mut::<8>()
            .0
            .iter_mut()
            .zip(mask.as_chunks::<2>().0)
        {
            pixel[6..].copy_from_slice(coverage);
        }
    }
}

pub(super) fn attach_alpha8(alpha: &frame::Video, target: &mut frame::Video) {
    let stride = target.stride(3);
    let mask_stride = alpha.stride(0);
    let width = target.width() as usize;

    for row in 0..target.height() as usize {
        target.data_mut(3)[row * stride..row * stride + width]
            .copy_from_slice(&alpha.data(0)[row * mask_stride..row * mask_stride + width]);
    }
}

fn straight_rgb(pixel: &[u8]) -> [f64; 3] {
    let alpha = sample(&pixel[6..]);

    if alpha == 0 {
        return [0.0; 3];
    }

    let inverse = 1.0 / f64::from(alpha);

    [sample(pixel), sample(&pixel[2..]), sample(&pixel[4..])]
        .map(|value| f64::from(value.min(alpha)) * inverse)
}

/// Unassociate only in RGB during final quantization. Chroma's weighted sum
/// cancels the division by alpha, avoiding a full-frame straight RGB temporary.
pub(super) fn to_yuva_reference(
    rgb_frame: &frame::Video,
    source: &frame::Video,
    conversion: ColorConversion,
) -> Result<frame::Video> {
    let width = rgb_frame.width() as usize;
    let height = rgb_frame.height() as usize;
    let stride = rgb_frame.stride(0);
    let mut output = output_frame(source, width, height, conversion);
    let strides: [usize; 4] = std::array::from_fn(|plane| output.stride(plane));

    let pixels = rgb_frame.data(0);

    for y in 0..height {
        let row = &pixels[y * stride..y * stride + width * 8];
        let luma = &mut output.data_mut(0)[y * strides[0]..y * strides[0] + width];

        for (pixel, value) in row.as_chunks::<8>().0.iter().zip(luma) {
            *value = conversion.luma(straight_rgb(pixel));
        }

        let alpha = &mut output.data_mut(3)[y * strides[3]..y * strides[3] + width];

        for (pixel, value) in row.as_chunks::<8>().0.iter().zip(alpha) {
            *value = ((u32::from(sample(&pixel[6..])) + 128) / 257) as u8;
        }
    }

    // One small row workspace avoids repeatedly borrowing FFmpeg plane metadata
    // per pixel and keeps all writes in safe, disjoint row slices.
    let mut chroma = vec![[0_u8; 2]; width / 2];

    for y in 0..height / 2 {
        for (x, value) in chroma.iter_mut().enumerate() {
            let mut weighted = [0_u64; 3];
            let mut coverage = 0_u64;

            for dy in 0..2 {
                for dx in 0..2 {
                    let pixel = &pixels[(y * 2 + dy) * stride + (x * 2 + dx) * 8..][..8];
                    let alpha = sample(&pixel[6..]);
                    coverage += u64::from(alpha);

                    for (channel, sum) in weighted.iter_mut().enumerate() {
                        *sum += u64::from(sample(&pixel[channel * 2..]).min(alpha));
                    }
                }
            }

            let color = if coverage == 0 {
                [0.0; 3]
            } else {
                weighted.map(|value| value as f64 / coverage as f64)
            };
            *value = conversion.chroma(color);
        }

        for (value, pixel) in output.data_mut(1)[y * strides[1]..y * strides[1] + width / 2]
            .iter_mut()
            .zip(&chroma)
        {
            *value = pixel[0];
        }

        for (value, pixel) in output.data_mut(2)[y * strides[2]..y * strides[2] + width / 2]
            .iter_mut()
            .zip(&chroma)
        {
            *value = pixel[1];
        }
    }

    Ok(output)
}

fn output_frame(
    source: &frame::Video,
    width: usize,
    height: usize,
    conversion: ColorConversion,
) -> frame::Video {
    let mut output = frame::Video::new(Pixel::YUVA420P, width as u32, height as u32);
    copy_color_properties(source, &mut output);
    output.set_color_space(conversion.space);
    output.set_color_range(if conversion.full {
        Range::JPEG
    } else {
        Range::MPEG
    });
    // RGB coverage is reduced over centered 2×2 cells, independently of the
    // decoded source's original chroma siting. FFmpeg has no safe setter here.
    unsafe {
        (*output.as_mut_ptr()).chroma_location = ffi::AVChromaLocation::AVCHROMA_LOC_CENTER;
    }

    output
}

pub(super) fn to_yuva(
    rgb: &frame::Video,
    source: &frame::Video,
    conversion: ColorConversion,
    pool: Option<&ThreadPool>,
) -> Result<frame::Video> {
    let (width, height) = (rgb.width() as usize, rgb.height() as usize);
    let mut output = output_frame(source, width, height, conversion);
    let strides: [usize; 4] = std::array::from_fn(|plane| output.stride(plane));
    // SAFETY: output was freshly allocated above as writable YUVA420P. Its four
    // positive-stride planes are disjoint, owned by output, and not aliased.
    let [y, u, v, alpha] = output_planes(&mut output);

    let bands = |y: &mut [u8], u: &mut [u8], v: &mut [u8], alpha: &mut [u8], first: usize| {
        for row in 0..u.len() / strides[1] {
            super::kernels::Rows {
                pixels: &rgb.data(0)[(first + row) * 2 * rgb.stride(0)..],
                source_stride: rgb.stride(0),
                width,
                y: &mut y[row * 2 * strides[0]..(row + 1) * 2 * strides[0]],
                y_stride: strides[0],
                u: &mut u[row * strides[1]..(row + 1) * strides[1]],
                v: &mut v[row * strides[2]..(row + 1) * strides[2]],
                alpha: &mut alpha[row * 2 * strides[3]..(row + 1) * 2 * strides[3]],
                alpha_stride: strides[3],
                color: conversion,
            }
            .run();
        }
    };

    if let Some(pool) = pool {
        let rows = (height / 2).div_ceil(pool.current_num_threads().min(4));
        pool.install(|| {
            y.par_chunks_mut(rows * 2 * strides[0])
                .zip(u.par_chunks_mut(rows * strides[1]))
                .zip(v.par_chunks_mut(rows * strides[2]))
                .zip(alpha.par_chunks_mut(rows * 2 * strides[3]))
                .enumerate()
                .for_each(|(index, (((y, u), v), alpha))| bands(y, u, v, alpha, index * rows));
        });
    } else {
        bands(y, u, v, alpha, 0);
    }

    Ok(output)
}

fn output_planes(output: &mut frame::Video) -> [&mut [u8]; 4] {
    let pointers: [(*mut u8, usize); 4] = std::array::from_fn(|plane| {
        let bytes = output.data_mut(plane);

        (bytes.as_mut_ptr(), bytes.len())
    });
    // SAFETY: only called for the freshly allocated, uniquely owned YUVA frame
    // above. Disjoint plane slices cannot outlive that frame's exclusive borrow.
    pointers.map(|(pointer, length)| unsafe { std::slice::from_raw_parts_mut(pointer, length) })
}
