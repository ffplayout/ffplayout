use std::any::type_name;

use rten_simd::{
    Isa, Simd, SimdOp,
    isa::GenericIsa,
    ops::{Extend, IntOps, NarrowSaturate, NumOps},
};

pub(crate) struct Plane<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub x: usize,
    pub y: usize,
}

pub(crate) struct PlaneMut<'a> {
    pub data: &'a mut [u8],
    pub stride: usize,
    pub x: usize,
    pub y: usize,
}

pub(crate) fn blend_plane(
    destination: PlaneMut<'_>,
    source: Plane<'_>,
    alpha: Plane<'_>,
    width: usize,
    height: usize,
    opacity: u8,
) {
    BlendPlane {
        destination,
        source,
        alpha,
        width,
        height,
        opacity,
    }
    .dispatch();
}

struct BlendPlane<'a> {
    destination: PlaneMut<'a>,
    source: Plane<'a>,
    alpha: Plane<'a>,
    width: usize,
    height: usize,
    opacity: u8,
}

impl SimdOp for BlendPlane<'_> {
    type Output = ();

    #[inline(always)]
    fn eval<I: Isa>(self, isa: I) {
        let u8_ops = isa.u8();
        let i16_ops = isa.i16();
        let lanes = u8_ops.len();
        let generic_fallback = type_name::<I>() == type_name::<GenericIsa>();

        for row in 0..self.height {
            let destination_start =
                (self.destination.y + row) * self.destination.stride + self.destination.x;
            let source_start = (self.source.y + row) * self.source.stride + self.source.x;
            let alpha_start = (self.alpha.y + row) * self.alpha.stride + self.alpha.x;

            let Some(destination) = self
                .destination
                .data
                .get_mut(destination_start..destination_start + self.width)
            else {
                continue;
            };
            let Some(source) = self
                .source
                .data
                .get(source_start..source_start + self.width)
            else {
                continue;
            };
            let Some(alpha) = self.alpha.data.get(alpha_start..alpha_start + self.width) else {
                continue;
            };

            if generic_fallback {
                // rten-simd 0.24's GenericIsa narrowing indexes beyond its
                // source vectors, so unsupported CPUs use the scalar fallback.
                blend_scalar(destination, source, alpha, self.opacity);
                continue;
            }

            let vectorized_len = self.width / lanes * lanes;
            for offset in (0..vectorized_len).step_by(lanes) {
                let destination_bytes = u8_ops.load(&destination[offset..]);
                let source_bytes = u8_ops.load(&source[offset..]);
                let alpha_bytes = u8_ops.load(&alpha[offset..]);

                let (destination_low, destination_high) = u8_ops.extend(destination_bytes);
                let (source_low, source_high) = u8_ops.extend(source_bytes);
                let (alpha_low, alpha_high) = u8_ops.extend(alpha_bytes);

                let blended_low =
                    blend_words(isa, destination_low, source_low, alpha_low, self.opacity);
                let blended_high =
                    blend_words(isa, destination_high, source_high, alpha_high, self.opacity);
                let blended = i16_ops.narrow_saturate(
                    blended_low.reinterpret_cast(),
                    blended_high.reinterpret_cast(),
                );
                u8_ops.store(blended, &mut destination[offset..]);
            }

            blend_scalar(
                &mut destination[vectorized_len..],
                &source[vectorized_len..],
                &alpha[vectorized_len..],
                self.opacity,
            );
        }
    }
}

#[inline(always)]
fn blend_words<I: Isa>(
    isa: I,
    destination: I::U16,
    source: I::U16,
    alpha: I::U16,
    opacity: u8,
) -> I::U16 {
    let ops = isa.u16();
    let opacity = ops.splat(u16::from(opacity));
    let rounding = ops.splat(127);
    let max_alpha = ops.splat(255);
    let effective_alpha = div_255(ops, ops.add(ops.mul(alpha, opacity), rounding));
    let inverse_alpha = ops.sub(max_alpha, effective_alpha);
    div_255(
        ops,
        ops.add(
            ops.add(
                ops.mul(destination, inverse_alpha),
                ops.mul(source, effective_alpha),
            ),
            rounding,
        ),
    )
}

#[inline(always)]
fn div_255<T: IntOps<u16>>(ops: T, value: T::Simd) -> T::Simd {
    let adjusted = ops.add(value, ops.one());
    ops.shift_right::<8>(ops.add(adjusted, ops.shift_right::<8>(adjusted)))
}

#[inline]
pub(crate) fn blend_scalar(destination: &mut [u8], source: &[u8], alpha: &[u8], opacity: u8) {
    for ((destination, source), source_alpha) in destination.iter_mut().zip(source).zip(alpha) {
        let alpha = mul_alpha(*source_alpha, opacity);
        if alpha == 0 {
            continue;
        }
        *destination = blend_u8(*destination, *source, alpha);
    }
}

#[inline]
fn blend_u8(destination: u8, source: u8, alpha: u8) -> u8 {
    let alpha = u16::from(alpha);
    ((u16::from(destination) * (255 - alpha) + u16::from(source) * alpha + 127) / 255) as u8
}

#[inline]
fn mul_alpha(source_alpha: u8, opacity: u8) -> u8 {
    ((u16::from(source_alpha) * u16::from(opacity) + 127) / 255) as u8
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn simd_matches_scalar_with_strides_offsets_and_tails() {
        for width in [1, 15, 16, 17, 31, 32, 33, 50, 320] {
            for opacity in [1, 127, 179, 255] {
                compare(width, 3, opacity);
            }
        }
    }

    fn compare(width: usize, height: usize, opacity: u8) {
        let source_stride = width + 7;
        let destination_stride = width + 11;
        let alpha_stride = width + 5;
        let source = generated_bytes(source_stride * (height + 2), 0x243f_6a88);
        let alpha = generated_bytes(alpha_stride * (height + 2), 0x85a3_08d3);
        let mut expected = generated_bytes(destination_stride * (height + 2), 0x1319_8a2e);
        let mut actual = expected.clone();
        let mut generic = expected.clone();

        for row in 0..height {
            let destination_start = (row + 1) * destination_stride + 3;
            let source_start = (row + 1) * source_stride + 2;
            let alpha_start = (row + 1) * alpha_stride + 1;
            blend_scalar(
                &mut expected[destination_start..destination_start + width],
                &source[source_start..source_start + width],
                &alpha[alpha_start..alpha_start + width],
                opacity,
            );
        }

        blend_plane(
            PlaneMut {
                data: &mut actual,
                stride: destination_stride,
                x: 3,
                y: 1,
            },
            Plane {
                data: &source,
                stride: source_stride,
                x: 2,
                y: 1,
            },
            Plane {
                data: &alpha,
                stride: alpha_stride,
                x: 1,
                y: 1,
            },
            width,
            height,
            opacity,
        );

        assert_eq!(actual, expected, "width {width}, opacity {opacity}");

        BlendPlane {
            destination: PlaneMut {
                data: &mut generic,
                stride: destination_stride,
                x: 3,
                y: 1,
            },
            source: Plane {
                data: &source,
                stride: source_stride,
                x: 2,
                y: 1,
            },
            alpha: Plane {
                data: &alpha,
                stride: alpha_stride,
                x: 1,
                y: 1,
            },
            width,
            height,
            opacity,
        }
        .eval(rten_simd::isa::GenericIsa::new());

        assert_eq!(
            generic, expected,
            "generic fallback: width {width}, opacity {opacity}"
        );
    }

    fn generated_bytes(len: usize, seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect()
    }
}
