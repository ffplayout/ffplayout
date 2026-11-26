//! Portable SIMD straight-YUVA quantization from associated RGB16.
use rten_simd::{
    Isa, Simd, SimdOp,
    ops::{BitOps, FloatOps, NumOps},
};

use super::{color::ColorConversion, rgba::sample};

#[cfg(all(target_arch = "aarch64", target_endian = "little"))]
mod neon;

const MAX_LANES: usize = 16;

pub(super) struct Rows<'a> {
    pub pixels: &'a [u8],
    pub source_stride: usize,
    pub width: usize,
    pub y: &'a mut [u8],
    pub y_stride: usize,
    pub u: &'a mut [u8],
    pub v: &'a mut [u8],
    pub alpha: &'a mut [u8],
    pub alpha_stride: usize,
    pub color: ColorConversion,
}

impl Rows<'_> {
    pub fn run(self) {
        #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
        if std::arch::is_aarch64_feature_detected!("neon") {
            // SAFETY: runtime detection guards all NEON instructions.
            unsafe {
                neon::run(self);
            }
            return;
        }

        self.portable();
    }

    pub(super) fn portable(self) {
        self.dispatch();
    }
}

#[inline(always)]
fn quantize<I: Isa>(isa: I, value: I::F32) -> I::I32 {
    let ops = isa.f32();

    ops.to_int_trunc(ops.add(
        ops.min(ops.max(value, ops.zero()), ops.splat(255.0)),
        ops.splat(0.5),
    ))
}

impl SimdOp for Rows<'_> {
    type Output = ();

    #[inline(always)]
    fn eval<I: Isa>(self, isa: I) {
        let ops = isa.f32();
        let lanes = ops.len();
        assert!(lanes <= MAX_LANES);
        let kr = ops.splat(self.color.kr as f32);
        let kb = ops.splat(self.color.kb as f32);
        let kg = ops.splat((1.0 - self.color.kr - self.color.kb) as f32);
        let y_scale = ops.splat(if self.color.full { 255.0 } else { 219.0 });
        let y_offset = ops.splat(if self.color.full { 0.0 } else { 16.0 });
        let c_scale = if self.color.full { 255.0 } else { 224.0 };
        let cb_scale = ops.splat((c_scale / (2.0 * (1.0 - self.color.kb))) as f32);
        let cr_scale = ops.splat((c_scale / (2.0 * (1.0 - self.color.kr))) as f32);

        for first in (0..self.width / 2).step_by(lanes) {
            let count = lanes.min(self.width / 2 - first);
            let mut sums = [ops.zero(); 4];

            for dy in 0..2 {
                for dx in 0..2 {
                    let mut input = [[0.0_f32; MAX_LANES]; 4];

                    for lane in 0..count {
                        let x = (first + lane) * 2 + dx;
                        let pixel = &self.pixels[dy * self.source_stride + x * 8..][..8];
                        let alpha = sample(&pixel[6..]);
                        input[3][lane] = f32::from(alpha);

                        for (channel, values) in input[..3].iter_mut().enumerate() {
                            values[lane] = f32::from(sample(&pixel[channel * 2..]).min(alpha));
                        }
                    }

                    let channels = input.map(|values| ops.load(&values));
                    let weighted = ops.add(
                        ops.add(ops.mul(channels[0], kr), ops.mul(channels[1], kg)),
                        ops.mul(channels[2], kb),
                    );
                    let luma = quantize(
                        isa,
                        ops.add(
                            y_offset,
                            ops.mul(y_scale, ops.div(weighted, ops.max(channels[3], ops.one()))),
                        ),
                    )
                    .to_array();
                    let alpha =
                        quantize(isa, ops.mul(channels[3], ops.splat(1.0 / 257.0))).to_array();

                    for lane in 0..count {
                        let x = (first + lane) * 2 + dx;
                        self.y[dy * self.y_stride + x] = luma[lane] as u8;
                        self.alpha[dy * self.alpha_stride + x] = alpha[lane] as u8;
                    }

                    for channel in 0..4 {
                        sums[channel] = ops.add(sums[channel], channels[channel]);
                    }
                }
            }

            let weighted = ops.add(
                ops.add(ops.mul(sums[0], kr), ops.mul(sums[1], kg)),
                ops.mul(sums[2], kb),
            );
            let denominator = ops.max(sums[3], ops.one());
            let cb = quantize(
                isa,
                ops.add(
                    ops.splat(128.0),
                    ops.mul(cb_scale, ops.div(ops.sub(sums[2], weighted), denominator)),
                ),
            )
            .to_array();
            let cr = quantize(
                isa,
                ops.add(
                    ops.splat(128.0),
                    ops.mul(cr_scale, ops.div(ops.sub(sums[0], weighted), denominator)),
                ),
            )
            .to_array();

            for lane in 0..count {
                self.u[first + lane] = cb[lane] as u8;
                self.v[first + lane] = cr[lane] as u8;
            }
        }
    }
}
