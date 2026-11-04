//! Fused high-precision SIMD YUV420 -> linear RGB -> over -> YUV420.
//! Only the final output is quantized to bytes. Scalar indexed LUT reads are
//! intentional: NEON has no arbitrary f32 gather. Matrix arithmetic, transfer
//! indices, blending and final Y quantization run across several pixels.
use rten_simd::{
    Isa, Simd, SimdOp,
    ops::{BitOps, Extend, FloatOps, Interleave, NumOps, ToFloat},
};

use rayon::{ThreadPool, prelude::*};

use super::color::{Coefficients, Color, Rect};

const MAX_LANES: usize = 16;

#[derive(Clone, Copy)]
pub struct Overlay<'a> {
    pub width: usize,
    pub height: usize,
    pub roi: Rect,
    pub color: Color,
    pub linear_planar: &'a [f32],
    pub decode_fine: &'a [f32],
    pub encode_fine: &'a [f32],
}

/// Apply one prepared overlay directly to an existing program frame. Each row
/// pair reads its own Y/U/V samples before replacing them; no extra frame copy
/// or RGB intermediate is needed between independent overlay passes.
pub fn blend_in_place(data: Overlay<'_>, output: &mut [u8], pool: &ThreadPool) {
    let pixels = data.width * data.height;
    let (y, uv) = output.split_at_mut(pixels);
    let (u, v) = uv.split_at_mut(pixels / 4);
    pool.install(|| {
        // Partition only affected rows so short overlays can use all workers.
        let first = data.roi.y;
        let end = first + data.roi.h;
        let chroma_first = first / 2 * (data.width / 2);
        let chroma_end = end / 2 * (data.width / 2);
        y[first * data.width..end * data.width]
            .par_chunks_mut(data.width * 2)
            .zip(u[chroma_first..chroma_end].par_chunks_mut(data.width / 2))
            .zip(v[chroma_first..chroma_end].par_chunks_mut(data.width / 2))
            .enumerate()
            .for_each(|(row, ((y, u), v))| {
                Rows {
                    data,
                    video: None,
                    row: first + row * 2,
                    y,
                    u,
                    v,
                }
                .run();
            });
    });
}

pub struct Rows<'a> {
    pub data: Overlay<'a>,
    /// None means read from the mutable row buffers before writing them back.
    pub video: Option<&'a [u8]>,
    pub row: usize,
    /// Two complete luma rows and one U/V row, with their original strides.
    pub y: &'a mut [u8],
    pub u: &'a mut [u8],
    pub v: &'a mut [u8],
}

impl Rows<'_> {
    // Keep identical kernel code for direct and thread-pool benchmark callers.
    #[inline(never)]
    pub fn run(self) {
        self.dispatch();
    }
}

#[inline(always)]
fn bytes_to_float<I: Isa>(isa: I, bytes: I::U8) -> I::F32 {
    let words: I::I16 = isa.u8().extend_low(bytes).reinterpret_cast();

    isa.i32().to_float(isa.i16().extend_low(words))
}

#[inline(always)]
fn lookup<I: Isa>(isa: I, value: I::F32, table: &[f32]) -> I::F32 {
    let ops = isa.f32();
    let value = ops.min(ops.max(value, ops.zero()), ops.one());
    let index = ops
        .to_int_trunc(ops.add(ops.mul(value, ops.splat(65535.0)), ops.splat(0.5)))
        .to_array();
    let mut values = [0.0; MAX_LANES];
    for lane in 0..ops.len() {
        values[lane] = table[index[lane] as usize];
    }

    ops.load(&values)
}

struct RowInput<I: Isa> {
    y: I::F32,
    u: I::F32,
    v: I::F32,
    foreground: [I::F32; 3],
    inverse: I::F32,
}

#[inline(always)]
fn compose<I: Isa>(
    isa: I,
    input: RowInput<I>,
    coefficients: Coefficients,
    data: Overlay<'_>,
) -> [I::F32; 3] {
    let ops = isa.f32();
    let kr = ops.splat(coefficients.kr);
    let kb = ops.splat(coefficients.kb);
    let kg = ops.splat(1.0 - coefficients.kr - coefficients.kb);
    let y = ops.mul(
        ops.sub(input.y, ops.splat(coefficients.y_offset)),
        ops.splat(1.0 / coefficients.y_scale),
    );
    let u = ops.mul(
        ops.sub(input.u, ops.splat(128.0)),
        ops.splat(1.0 / coefficients.c_scale),
    );
    let v = ops.mul(
        ops.sub(input.v, ops.splat(128.0)),
        ops.splat(1.0 / coefficients.c_scale),
    );
    let r = ops.add(y, ops.mul(v, ops.splat(2.0 * (1.0 - coefficients.kr))));
    let b = ops.add(y, ops.mul(u, ops.splat(2.0 * (1.0 - coefficients.kb))));
    let g = ops.div(ops.sub(ops.sub(y, ops.mul(kr, r)), ops.mul(kb, b)), kg);
    let unchanged = ops.eq(input.inverse, ops.one());
    let mut rgb = [r, g, b];

    for (channel, value) in rgb.iter_mut().enumerate() {
        let base = ops.min(ops.max(*value, ops.zero()), ops.one());
        let linear = lookup(isa, base, data.decode_fine);
        let blended = ops.add(input.foreground[channel], ops.mul(linear, input.inverse));
        *value = ops.select(base, lookup(isa, blended, data.encode_fine), unchanged);
    }

    let [r, g, b] = rgb;
    let y = ops.add(ops.add(ops.mul(kr, r), ops.mul(kg, g)), ops.mul(kb, b));

    [
        ops.add(
            ops.splat(coefficients.y_offset),
            ops.mul(y, ops.splat(coefficients.y_scale)),
        ),
        ops.add(
            ops.splat(128.0),
            ops.mul(
                ops.sub(b, y),
                ops.splat(coefficients.c_scale / (2.0 * (1.0 - coefficients.kb))),
            ),
        ),
        ops.add(
            ops.splat(128.0),
            ops.mul(
                ops.sub(r, y),
                ops.splat(coefficients.c_scale / (2.0 * (1.0 - coefficients.kr))),
            ),
        ),
    ]
}

impl SimdOp for Rows<'_> {
    type Output = ();

    #[inline(always)]
    fn eval<I: Isa>(self, isa: I) {
        let ops = isa.f32();
        let bytes = isa.u8();
        let lanes = ops.len();
        assert!(lanes <= MAX_LANES && lanes.is_multiple_of(2));
        let data = self.data;
        let rect = data.roi;
        let area = rect.w * rect.h;
        let pixels = data.width * data.height;
        let coefficients = data.color.coefficients();

        for x in (rect.x..rect.x + rect.w).step_by(lanes) {
            // The ROI is 2x2 aligned. Even the short SIMD tail contains whole
            // chroma cells; padded lanes are never written to output.
            let count = lanes.min(rect.x + rect.w - x);
            let chroma = self.row / 2 * (data.width / 2) + x / 2;
            let u_source = self
                .video
                .map_or(&self.u[x / 2..x / 2 + count / 2], |video| {
                    &video[pixels + chroma..pixels + chroma + count / 2]
                });
            let v_source = self
                .video
                .map_or(&self.v[x / 2..x / 2 + count / 2], |video| {
                    &video[pixels + pixels / 4 + chroma..pixels + pixels / 4 + chroma + count / 2]
                });
            let u_bytes = bytes.load_pad(u_source).0;
            let v_bytes = bytes.load_pad(v_source).0;
            let u = bytes_to_float(isa, bytes.interleave_low(u_bytes, u_bytes));
            let v = bytes_to_float(isa, bytes.interleave_low(v_bytes, v_bytes));
            let mut chroma_sum = [ops.zero(); 2];

            for dy in 0..2 {
                let source = (self.row + dy) * data.width + x;
                let foreground = (self.row + dy - rect.y) * rect.w + x - rect.x;
                let y_source = self.video.map_or(
                    &self.y[dy * data.width + x..dy * data.width + x + count],
                    |video| &video[source..source + count],
                );
                let input = RowInput {
                    y: bytes_to_float(isa, bytes.load_pad(y_source).0),
                    u,
                    v,
                    foreground: std::array::from_fn(|c| {
                        ops.load_pad(
                            &data.linear_planar
                                [c * area + foreground..c * area + foreground + count],
                        )
                        .0
                    }),
                    inverse: ops
                        .load_pad(
                            &data.linear_planar
                                [3 * area + foreground..3 * area + foreground + count],
                        )
                        .0,
                };
                let [y, u, v] = compose(isa, input, coefficients, data);
                let y = ops
                    .to_int_trunc(ops.add(
                        ops.min(ops.max(y, ops.zero()), ops.splat(255.0)),
                        ops.splat(0.5),
                    ))
                    .to_array();

                for lane in 0..count {
                    self.y[dy * data.width + x + lane] = y[lane] as u8;
                }

                chroma_sum[0] = ops.add(chroma_sum[0], u);
                chroma_sum[1] = ops.add(chroma_sum[1], v);
            }

            let u = chroma_sum[0].to_array();
            let v = chroma_sum[1].to_array();
            for lane in (0..count).step_by(2) {
                self.u[(x + lane) / 2] =
                    super::color::quantize(f64::from((u[lane] + u[lane + 1]) * 0.25));
                self.v[(x + lane) / 2] =
                    super::color::quantize(f64::from((v[lane] + v[lane + 1]) * 0.25));
            }
        }
    }
}
