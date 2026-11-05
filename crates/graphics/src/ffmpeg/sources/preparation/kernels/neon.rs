//! Eight packed RGBA16 pixels per load; no scalar channel-gather temporaries.
use std::arch::aarch64::*;

use super::Rows;

#[derive(Clone, Copy)]
struct Coefficients {
    kr: float32x4_t,
    kg: float32x4_t,
    kb: float32x4_t,
    y_scale: float32x4_t,
    y_offset: float32x4_t,
    cb_scale: float32x4_t,
    cr_scale: float32x4_t,
}

#[target_feature(enable = "neon")]
fn weighted(channels: [float32x4_t; 3], c: Coefficients) -> float32x4_t {
    vaddq_f32(
        vaddq_f32(vmulq_f32(channels[0], c.kr), vmulq_f32(channels[1], c.kg)),
        vmulq_f32(channels[2], c.kb),
    )
}

#[target_feature(enable = "neon")]
fn quantize(value: float32x4_t) -> uint32x4_t {
    vcvtq_u32_f32(vaddq_f32(
        vminq_f32(vmaxq_f32(value, vdupq_n_f32(0.0)), vdupq_n_f32(255.0)),
        vdupq_n_f32(0.5),
    ))
}

#[target_feature(enable = "neon")]
fn pack(low: uint32x4_t, high: uint32x4_t) -> uint8x8_t {
    vmovn_u16(vcombine_u16(vmovn_u32(low), vmovn_u32(high)))
}

#[target_feature(enable = "neon")]
fn luma(channels: [uint16x8_t; 4], c: Coefficients) -> uint8x8_t {
    let low = channels.map(|channel| vcvtq_f32_u32(vmovl_u16(vget_low_u16(channel))));
    let high = channels.map(|channel| vcvtq_f32_u32(vmovl_u16(vget_high_u16(channel))));
    let convert = |values: [float32x4_t; 4]| {
        let value = vdivq_f32(
            weighted([values[0], values[1], values[2]], c),
            vmaxq_f32(values[3], vdupq_n_f32(1.0)),
        );

        quantize(vaddq_f32(c.y_offset, vmulq_f32(c.y_scale, value)))
    };

    pack(convert(low), convert(high))
}

#[target_feature(enable = "neon")]
fn coverage(alpha: uint16x8_t) -> uint8x8_t {
    let scale = vdupq_n_f32(1.0 / 257.0);
    let low = vmulq_f32(vcvtq_f32_u32(vmovl_u16(vget_low_u16(alpha))), scale);
    let high = vmulq_f32(vcvtq_f32_u32(vmovl_u16(vget_high_u16(alpha))), scale);

    pack(quantize(low), quantize(high))
}

#[target_feature(enable = "neon")]
fn sum_cells(top: uint16x8_t, bottom: uint16x8_t) -> float32x4_t {
    let low = vaddl_u16(vget_low_u16(top), vget_low_u16(bottom));
    let high = vaddl_u16(vget_high_u16(top), vget_high_u16(bottom));

    vcvtq_f32_u32(vpaddq_u32(low, high))
}

/// Caller verifies NEON availability; each full block stays inside the visible
/// width. The remaining even-width tail uses the portable SIMD implementation.
#[target_feature(enable = "neon")]
pub(super) fn run(rows: Rows<'_>) {
    let color = rows.color;
    let c_scale = if color.full { 255.0 } else { 224.0 };
    let c = Coefficients {
        kr: vdupq_n_f32(color.kr as f32),
        kg: vdupq_n_f32((1.0 - color.kr - color.kb) as f32),
        kb: vdupq_n_f32(color.kb as f32),
        y_scale: vdupq_n_f32(if color.full { 255.0 } else { 219.0 }),
        y_offset: vdupq_n_f32(if color.full { 0.0 } else { 16.0 }),
        cb_scale: vdupq_n_f32((c_scale / (2.0 * (1.0 - color.kb))) as f32),
        cr_scale: vdupq_n_f32((c_scale / (2.0 * (1.0 - color.kr))) as f32),
    };
    let end = rows.width / 8 * 8;

    for x in (0..end).step_by(8) {
        // SAFETY: a block has eight visible RGBA16 pixels in each of two
        // validated rows. NEON supports these unaligned interleaved loads.
        let (top, bottom) = unsafe {
            (
                vld4q_u16(rows.pixels.as_ptr().add(x * 8).cast()),
                vld4q_u16(rows.pixels.as_ptr().add(rows.source_stride + x * 8).cast()),
            )
        };
        let top = [
            vminq_u16(top.0, top.3),
            vminq_u16(top.1, top.3),
            vminq_u16(top.2, top.3),
            top.3,
        ];
        let bottom = [
            vminq_u16(bottom.0, bottom.3),
            vminq_u16(bottom.1, bottom.3),
            vminq_u16(bottom.2, bottom.3),
            bottom.3,
        ];
        // SAFETY: full eight-byte stores stay inside their disjoint output rows.
        unsafe {
            vst1_u8(rows.y.as_mut_ptr().add(x), luma(top, c));
            vst1_u8(rows.y.as_mut_ptr().add(rows.y_stride + x), luma(bottom, c));
            vst1_u8(rows.alpha.as_mut_ptr().add(x), coverage(top[3]));
            vst1_u8(
                rows.alpha.as_mut_ptr().add(rows.alpha_stride + x),
                coverage(bottom[3]),
            );
        }

        let sums =
            std::array::from_fn::<_, 4, _>(|channel| sum_cells(top[channel], bottom[channel]));
        let weighted = weighted([sums[0], sums[1], sums[2]], c);
        let denominator = vmaxq_f32(sums[3], vdupq_n_f32(1.0));
        let cb = quantize(vaddq_f32(
            vdupq_n_f32(128.0),
            vmulq_f32(
                c.cb_scale,
                vdivq_f32(vsubq_f32(sums[2], weighted), denominator),
            ),
        ));
        let cr = quantize(vaddq_f32(
            vdupq_n_f32(128.0),
            vmulq_f32(
                c.cr_scale,
                vdivq_f32(vsubq_f32(sums[0], weighted), denominator),
            ),
        ));
        let cb = vmovn_u16(vcombine_u16(vmovn_u32(cb), vdup_n_u16(0)));
        let cr = vmovn_u16(vcombine_u16(vmovn_u32(cr), vdup_n_u16(0)));
        let mut u = [0_u8; 8];
        let mut v = [0_u8; 8];
        // SAFETY: each local array holds the complete eight-byte vector store.
        unsafe {
            vst1_u8(u.as_mut_ptr(), cb);
            vst1_u8(v.as_mut_ptr(), cr);
        }
        rows.u[x / 2..x / 2 + 4].copy_from_slice(&u[..4]);
        rows.v[x / 2..x / 2 + 4].copy_from_slice(&v[..4]);
    }

    if end < rows.width {
        Rows {
            pixels: &rows.pixels[end * 8..],
            source_stride: rows.source_stride,
            width: rows.width - end,
            y: &mut rows.y[end..],
            y_stride: rows.y_stride,
            u: &mut rows.u[end / 2..],
            v: &mut rows.v[end / 2..],
            alpha: &mut rows.alpha[end..],
            alpha_stride: rows.alpha_stride,
            color: rows.color,
        }
        .portable();
    }
}
