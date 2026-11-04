use std::any::type_name;

use rten_simd::{
    Isa, Simd, SimdOp,
    isa::GenericIsa,
    ops::{BitOps, Extend, IntOps, NarrowSaturate, NumOps},
};

/// Static premultiplied source retains sub-byte precision (code * alpha in
/// 0..65025). Alpha is NOT multiplied a second time during compositing.
pub fn over_scalar(dst: &mut [u8], premul: &[u16], alpha: &[u8]) {
    for ((d, &s), &a) in dst.iter_mut().zip(premul).zip(alpha) {
        *d = ((u32::from(s) + u32::from(*d) * (255 - u32::from(a)) + 127) / 255).min(255) as u8;
    }
}

pub fn over_simd(dst: &mut [u8], premul: &[u16], alpha: &[u8]) {
    Over { dst, premul, alpha }.dispatch();
}

struct Over<'a> {
    dst: &'a mut [u8],
    premul: &'a [u16],
    alpha: &'a [u8],
}

impl SimdOp for Over<'_> {
    type Output = ();

    #[inline(always)]
    fn eval<I: Isa>(self, isa: I) {
        if type_name::<I>() == type_name::<GenericIsa>() {
            over_scalar(self.dst, self.premul, self.alpha);
            return;
        }

        let bytes = isa.u8();
        let words = isa.u16();
        let lanes = bytes.len();
        let half = words.len();
        let end = self.dst.len() / lanes * lanes;

        for offset in (0..end).step_by(lanes) {
            let d = bytes.load(&self.dst[offset..]);
            let a = bytes.load(&self.alpha[offset..]);
            let compose = |d, a, s| {
                let sum = words.add(words.mul(d, words.sub(words.splat(255), a)), s);
                let rounded = words.add(sum, words.splat(128));
                // Exact rounded /255 for the bounded unsigned numerator.
                words.shift_right::<8>(words.add(rounded, words.shift_right::<8>(rounded)))
            };
            let low = compose(
                bytes.extend_low(d),
                bytes.extend_low(a),
                words.load(&self.premul[offset..]),
            );
            let high = compose(
                bytes.extend_high(d),
                bytes.extend_high(a),
                words.load(&self.premul[offset + half..]),
            );
            let result = isa
                .i16()
                .narrow_saturate(low.reinterpret_cast(), high.reinterpret_cast());
            bytes.store(result, &mut self.dst[offset..]);
        }

        over_scalar(
            &mut self.dst[end..],
            &self.premul[end..],
            &self.alpha[end..],
        );
    }
}

pub fn over_float(dst: &mut [f32], source: &[f32], inverse: &[f32]) {
    FloatOver {
        dst,
        source,
        inverse,
    }
    .dispatch();
}

struct FloatOver<'a> {
    dst: &'a mut [f32],
    source: &'a [f32],
    inverse: &'a [f32],
}

impl SimdOp for FloatOver<'_> {
    type Output = ();

    #[inline(always)]
    fn eval<I: Isa>(self, isa: I) {
        let ops = isa.f32();
        let lanes = ops.len();
        let end = self.dst.len() / lanes * lanes;

        for i in (0..end).step_by(lanes) {
            let blended = ops.add(
                ops.load(&self.source[i..]),
                ops.mul(ops.load(&self.dst[i..]), ops.load(&self.inverse[i..])),
            );
            ops.store(blended, &mut self.dst[i..]);
        }

        for ((d, s), inv) in self.dst[end..]
            .iter_mut()
            .zip(&self.source[end..])
            .zip(&self.inverse[end..])
        {
            *d = *s + *d * inv;
        }
    }
}

pub fn backend() -> String {
    Backend.dispatch()
}
struct Backend;
impl SimdOp for Backend {
    type Output = String;
    fn eval<I: Isa>(self, isa: I) -> String {
        format!(
            "{} ({} byte lanes / {} float lanes)",
            type_name::<I>(),
            isa.u8().len(),
            isa.f32().len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplied_simd_matches_scalar_including_tail_and_all_alphas() {
        for len in [1, 15, 31, 65, 1027] {
            let alpha: Vec<_> = (0..len).map(|i| i as u8).collect();
            let source: Vec<_> = alpha
                .iter()
                .enumerate()
                .map(|(i, a)| u16::from(*a) * u16::from((i * 31) as u8))
                .collect();
            let mut expected: Vec<_> = (0..len).map(|i| (i * 79) as u8).collect();
            let mut actual = expected.clone();
            over_scalar(&mut expected, &source, &alpha);
            over_simd(&mut actual, &source, &alpha);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn zero_alpha_and_opaque_alpha_have_exact_endpoints() {
        let mut actual = [19, 200];
        over_simd(&mut actual, &[0, 40 * 255], &[0, 255]);
        assert_eq!(actual, [19, 40]);
    }
}
