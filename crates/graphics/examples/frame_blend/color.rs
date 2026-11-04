//! Explicit SDR color model. Raw YUV has no metadata: never infer these values
//! from its filename. Chroma reconstruction is nearest-neighbour for every path;
//! final 4:2:0 reduction is a 2x2 box, so conversion quality is comparable.
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Matrix {
    Bt709,
    Bt601,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Range {
    Limited,
    Full,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Transfer {
    Bt709,
    Srgb,
}

impl Transfer {
    pub fn decode(self, value: f64) -> f64 {
        let v = value.clamp(0.0, 1.0);
        match self {
            Self::Srgb if v <= 0.04045 => v / 12.92,
            Self::Srgb => ((v + 0.055) / 1.055).powf(2.4),
            Self::Bt709 if v < 0.081 => v / 4.5,
            Self::Bt709 => ((v + 0.099) / 1.099).powf(1.0 / 0.45),
        }
    }

    pub fn encode(self, value: f64) -> f64 {
        let v = value.clamp(0.0, 1.0);
        match self {
            Self::Srgb if v <= 0.0031308 => 12.92 * v,
            Self::Srgb => 1.055 * v.powf(1.0 / 2.4) - 0.055,
            Self::Bt709 if v < 0.018 => 4.5 * v,
            Self::Bt709 => 1.099 * v.powf(0.45) - 0.099,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Color {
    pub matrix: Matrix,
    pub range: Range,
    pub transfer: Transfer,
    kr: f64,
    kb: f64,
    pub y_offset: f64,
    y_scale: f64,
    c_scale: f64,
}

#[derive(Clone, Copy)]
pub struct Coefficients {
    pub kr: f32,
    pub kb: f32,
    pub y_offset: f32,
    pub y_scale: f32,
    pub c_scale: f32,
}

impl Color {
    pub fn coefficients(self) -> Coefficients {
        Coefficients {
            kr: self.kr as f32,
            kb: self.kb as f32,
            y_offset: self.y_offset as f32,
            y_scale: self.y_scale as f32,
            c_scale: self.c_scale as f32,
        }
    }

    pub fn new(matrix: Matrix, range: Range, transfer: Transfer) -> Self {
        let (kr, kb) = match matrix {
            Matrix::Bt709 => (0.2126, 0.0722),
            Matrix::Bt601 => (0.299, 0.114),
        };
        let (y_offset, y_scale, c_scale) = match range {
            Range::Limited => (16.0, 219.0, 224.0),
            Range::Full => (0.0, 255.0, 255.0),
        };

        Self {
            matrix,
            range,
            transfer,
            kr,
            kb,
            y_offset,
            y_scale,
            c_scale,
        }
    }

    pub fn rgb(self, y: u8, u: u8, v: u8) -> [f64; 3] {
        let y = (f64::from(y) - self.y_offset) / self.y_scale;
        let u = (f64::from(u) - 128.0) / self.c_scale;
        let v = (f64::from(v) - 128.0) / self.c_scale;
        let r = y + 2.0 * (1.0 - self.kr) * v;
        let b = y + 2.0 * (1.0 - self.kb) * u;
        let g = (y - self.kr * r - self.kb * b) / (1.0 - self.kr - self.kb);

        [r, g, b].map(|v| v.clamp(0.0, 1.0))
    }

    pub fn yuv(self, rgb: [f64; 3]) -> [f64; 3] {
        let [r, g, b] = rgb;
        let y = self.kr * r + (1.0 - self.kr - self.kb) * g + self.kb * b;

        [
            self.y_offset + self.y_scale * y,
            128.0 + self.c_scale * (b - y) / (2.0 * (1.0 - self.kb)),
            128.0 + self.c_scale * (r - y) / (2.0 * (1.0 - self.kr)),
        ]
    }
}

pub fn quantize(value: f64) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl Rect {
    pub fn full(w: usize, h: usize) -> Self {
        Self { x: 0, y: 0, w, h }
    }

    pub fn chroma(self) -> Self {
        Self {
            x: self.x / 2,
            y: self.y / 2,
            w: self.w / 2,
            h: self.h / 2,
        }
    }
}

pub fn source_rgb(
    data: &[u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    color: Color,
) -> [f64; 3] {
    let pixels = width * height;
    let uv = y / 2 * (width / 2) + x / 2;

    color.rgb(
        data[y * width + x],
        data[pixels + uv],
        data[pixels + pixels / 4 + uv],
    )
}

/// Transform and compose a whole 2x2 block before averaging chroma. Alpha is
/// scalar coverage, never gamma encoded. Used by the f64 quality reference and
/// the fused sparse linear path (with an explicitly quantized LUT when enabled).
pub fn compose_blocks(
    output: &mut [u8],
    width: usize,
    height: usize,
    rect: Rect,
    color: Color,
    mut pixel: impl FnMut(usize, usize) -> [f64; 3],
) {
    let pixels = width * height;

    for y in (rect.y..rect.y + rect.h).step_by(2) {
        for x in (rect.x..rect.x + rect.w).step_by(2) {
            let mut chroma = [0.0; 2];

            for dy in 0..2 {
                for dx in 0..2 {
                    let values = color.yuv(pixel(x + dx, y + dy));
                    output[(y + dy) * width + x + dx] = quantize(values[0]);
                    chroma[0] += values[1];
                    chroma[1] += values[2];
                }
            }

            let uv = y / 2 * (width / 2) + x / 2;
            output[pixels + uv] = quantize(chroma[0] / 4.0);
            output[pixels + pixels / 4 + uv] = quantize(chroma[1] / 4.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_black_and_white_and_neutral_chroma_round_trip() {
        let color = Color::new(Matrix::Bt709, Range::Limited, Transfer::Bt709);
        for y in [16, 100, 235] {
            assert_eq!(
                color.yuv(color.rgb(y, 128, 128)).map(quantize),
                [y, 128, 128]
            );
        }
    }

    #[test]
    fn transfer_curves_round_trip_and_linear_blend_is_brighter() {
        for transfer in [Transfer::Srgb, Transfer::Bt709] {
            for code in [0.0, 0.03, 0.3, 0.7, 1.0] {
                assert!((transfer.encode(transfer.decode(code)) - code).abs() < 0.001);
            }
            assert!(transfer.encode(0.5) > 0.7);
        }
    }
}
