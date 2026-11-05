//! Pure RGB layout/depth operations. YUV conversion remains with swscale.
use ffmpeg_next::{frame, util::format::pixel::Pixel};
use rten_simd::{
    Isa, SimdOp,
    ops::{BitOps, Interleave},
};

use crate::ffmpeg::compositor::metadata::copy_color_properties;

pub(super) fn convert(source: &frame::Video) -> Option<frame::Video> {
    if !matches!(source.format(), Pixel::RGBA | Pixel::RGBA64LE | Pixel::GBRP) {
        return None;
    }

    let mut output = frame::Video::new(Pixel::RGBA64LE, source.width(), source.height());
    copy_color_properties(source, &mut output);
    let stride = output.stride(0);
    let width = source.width() as usize;

    for row in 0..source.height() as usize {
        let destination = &mut output.data_mut(0)[row * stride..row * stride + width * 8];

        match source.format() {
            Pixel::RGBA => Expand {
                source: &source.data(0)[row * source.stride(0)..row * source.stride(0) + width * 4],
                destination,
            }
            .dispatch(),
            Pixel::RGBA64LE => destination.copy_from_slice(
                &source.data(0)[row * source.stride(0)..row * source.stride(0) + width * 8],
            ),
            Pixel::GBRP => {
                let g = &source.data(0)[row * source.stride(0)..];
                let b = &source.data(1)[row * source.stride(1)..];
                let r = &source.data(2)[row * source.stride(2)..];

                for (x, pixel) in destination.as_chunks_mut::<8>().0.iter_mut().enumerate() {
                    *pixel = [r[x], r[x], g[x], g[x], b[x], b[x], 255, 255];
                }
            }
            _ => unreachable!("validated RGB layout"),
        }
    }

    Some(output)
}

struct Expand<'a> {
    source: &'a [u8],
    destination: &'a mut [u8],
}

impl SimdOp for Expand<'_> {
    type Output = ();

    #[inline(always)]
    fn eval<I: Isa>(self, isa: I) {
        let ops = isa.u8();
        let lanes = ops.len();
        let end = self.source.len() / lanes * lanes;

        for first in (0..end).step_by(lanes) {
            let pixels = ops.load(&self.source[first..]);
            // Byte replication is exactly value * 257, including 0 and 65535.
            ops.store(
                ops.interleave_low(pixels, pixels),
                &mut self.destination[first * 2..],
            );
            ops.store(
                ops.interleave_high(pixels, pixels),
                &mut self.destination[first * 2 + lanes..],
            );
        }

        for (source, destination) in self.source[end..]
            .iter()
            .zip(self.destination[end * 2..].as_chunks_mut::<2>().0)
        {
            *destination = [*source; 2];
        }
    }
}
