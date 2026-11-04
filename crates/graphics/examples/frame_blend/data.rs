use std::path::Path;

use anyhow::{Result, ensure};
use image::ImageReader;

use super::color::{Color, Rect, Transfer, compose_blocks, quantize, source_rgb};
use super::video_input::load_video;

pub struct Weighted {
    pub source: Vec<u16>,
    pub alpha: Vec<u8>,
}

impl Weighted {
    fn new(length: usize) -> Self {
        Self {
            source: vec![0; length],
            alpha: vec![0; length],
        }
    }
}

pub struct Data {
    pub width: usize,
    pub height: usize,
    pub video: Vec<u8>,
    pub graphic: Vec<u8>,
    pub rgba: Weighted,
    pub linear: Vec<f32>,
    pub inverse: Vec<f32>,
    pub yuva444: Weighted,
    pub yuva420: Weighted,
    pub roi: Rect,
    pub color: Color,
    pub opacity: f64,
    pub decode_lut: [f32; 256],
    pub encode_lut: Vec<u8>,
    pub decode_fine: Vec<f32>,
    pub encode_fine: Vec<f32>,
    /// ROI-sized SoA planes R/G/B (linear premultiplied) and inverse coverage.
    pub linear_planar: Vec<f32>,
}

impl Data {
    pub fn load(
        video: &Path,
        graphic: &Path,
        width: usize,
        height: usize,
        color: Color,
        opacity: f64,
        input_premultiplied: bool,
    ) -> Result<Self> {
        let video = load_video(video, width, height, color)?;
        let image = ImageReader::open(graphic)?.decode()?.into_rgba8();
        ensure!(
            image.dimensions() == (width as u32, height as u32),
            "graphic dimensions must match the video; no implicit rescaling"
        );
        let mut graphic = image.into_raw();

        if input_premultiplied {
            for pixel in graphic.as_chunks_mut::<4>().0 {
                let alpha = f64::from(pixel[3]);
                for value in &mut pixel[..3] {
                    *value = if alpha == 0.0 {
                        0
                    } else {
                        quantize(f64::from(*value) * 255.0 / alpha)
                    };
                }
            }
        }

        let pixels = width * height;
        let mut result = Self {
            width,
            height,
            video,
            graphic,
            rgba: Weighted::new(pixels * 4),
            linear: vec![0.0; pixels * 4],
            inverse: vec![1.0; pixels * 4],
            yuva444: Weighted::new(pixels * 3),
            yuva420: Weighted::new(pixels * 3 / 2),
            roi: Rect::full(width, height),
            color,
            opacity,
            decode_lut: std::array::from_fn(|i| color.transfer.decode(i as f64 / 255.0) as f32),
            encode_lut: (0..=65535)
                .map(|i| quantize(color.transfer.encode(f64::from(i) / 65535.0) * 255.0))
                .collect(),
            decode_fine: (0..=65535)
                .map(|i| color.transfer.decode(f64::from(i) / 65535.0) as f32)
                .collect(),
            encode_fine: (0..=65535)
                .map(|i| color.transfer.encode(f64::from(i) / 65535.0) as f32)
                .collect(),
            linear_planar: Vec::new(),
        };
        let mut min = [width, height];
        let mut max = [0, 0];

        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                if result.graphic[i * 4 + 3] == 0 {
                    continue;
                }
                let (encoded, linear, alpha) = result.graphic_pixel(i);
                let a = quantize(alpha * 255.0);

                if alpha > 0.0 {
                    min[0] = min[0].min(x);
                    min[1] = min[1].min(y);
                    max[0] = max[0].max(x + 1);
                    max[1] = max[1].max(y + 1);
                }

                for channel in 0..4 {
                    let code = if channel == 3 {
                        255
                    } else {
                        quantize(encoded[channel] * 255.0)
                    };
                    // Classic RGBA8 stores rounded premultiplied codes. Retain
                    // that quantization for comparison with tiny-skia.
                    result.rgba.source[i * 4 + channel] =
                        u16::from(quantize(f64::from(code) * f64::from(a) / 255.0)) * 255;
                    result.rgba.alpha[i * 4 + channel] = a;
                    result.linear[i * 4 + channel] = if channel == 3 {
                        alpha as f32
                    } else {
                        (linear[channel] * alpha) as f32
                    };
                    result.inverse[i * 4 + channel] = (1.0 - alpha) as f32;
                }

                let yuv = color.yuv(encoded);
                for (plane, code) in yuv.into_iter().enumerate() {
                    result.yuva444.source[plane * pixels + i] =
                        (code.clamp(0.0, 255.0) * f64::from(a)).round() as u16;
                    result.yuva444.alpha[plane * pixels + i] = a;
                }
                result.yuva420.source[i] = result.yuva444.source[i];
                result.yuva420.alpha[i] = a;
            }
        }

        for plane in 1..3 {
            let offset = pixels + (plane - 1) * pixels / 4;
            for y in 0..height / 2 {
                for x in 0..width / 2 {
                    let mut source = 0_u32;
                    let mut alpha = 0_u32;
                    for dy in 0..2 {
                        for dx in 0..2 {
                            let i = (y * 2 + dy) * width + x * 2 + dx;
                            source += u32::from(result.yuva444.source[plane * pixels + i]);
                            alpha += u32::from(result.yuva444.alpha[plane * pixels + i]);
                        }
                    }
                    // Alpha-weighted chroma: average premultiplied colors,
                    // NOT unassociated UV followed by an unrelated mean alpha.
                    result.yuva420.source[offset + y * (width / 2) + x] = ((source + 2) / 4) as u16;
                    result.yuva420.alpha[offset + y * (width / 2) + x] = ((alpha + 2) / 4) as u8;
                }
            }
        }

        ensure!(
            max[0] > min[0] && max[1] > min[1],
            "graphic has no nonzero effective alpha"
        );
        min = min.map(|value| value & !1);
        max = max.map(|value| (value + 1) & !1);
        result.roi = Rect {
            x: min[0],
            y: min[1],
            w: max[0] - min[0],
            h: max[1] - min[1],
        };
        let area = result.roi.w * result.roi.h;
        result.linear_planar.resize(area * 4, 0.0);
        for y in 0..result.roi.h {
            for x in 0..result.roi.w {
                let source = ((y + result.roi.y) * width + x + result.roi.x) * 4;
                let target = y * result.roi.w + x;
                for c in 0..3 {
                    result.linear_planar[c * area + target] = result.linear[source + c];
                }
                result.linear_planar[3 * area + target] = result.inverse[source];
            }
        }

        Ok(result)
    }

    pub fn graphic_pixel(&self, i: usize) -> ([f64; 3], [f64; 3], f64) {
        let pixel = &self.graphic[i * 4..i * 4 + 4];
        let linear = std::array::from_fn(|c| Transfer::Srgb.decode(f64::from(pixel[c]) / 255.0));

        (
            linear.map(|v| self.color.transfer.encode(v)),
            linear,
            f64::from(pixel[3]) / 255.0 * self.opacity,
        )
    }

    pub fn reference(&self, linear: bool) -> Vec<u8> {
        let mut result = self.video.clone();
        compose_blocks(
            &mut result,
            self.width,
            self.height,
            self.roi,
            self.color,
            |x, y| {
                let base = source_rgb(&self.video, self.width, self.height, x, y, self.color);
                let (encoded, source, alpha) = self.graphic_pixel(y * self.width + x);

                if linear {
                    std::array::from_fn(|c| {
                        self.color.transfer.encode(
                            source[c] * alpha + self.color.transfer.decode(base[c]) * (1.0 - alpha),
                        )
                    })
                } else {
                    std::array::from_fn(|c| encoded[c] * alpha + base[c] * (1.0 - alpha))
                }
            },
        );

        result
    }

    /// Independent f64 native-YUV reference. Unlike an RGB round-trip, this
    /// deliberately preserves out-of-RGB-gamut input YUV values. Average
    /// alpha-weighted full-resolution chroma before final quantization.
    pub fn reference_yuv(&self) -> Vec<u8> {
        let mut output = self.video.clone();
        let pixels = self.width * self.height;
        for y in (self.roi.y..self.roi.y + self.roi.h).step_by(2) {
            for x in (self.roi.x..self.roi.x + self.roi.w).step_by(2) {
                let uv = y / 2 * (self.width / 2) + x / 2;
                let background = [
                    0.0,
                    f64::from(self.video[pixels + uv]),
                    f64::from(self.video[pixels + pixels / 4 + uv]),
                ];
                let mut chroma = [0.0; 2];
                for dy in 0..2 {
                    for dx in 0..2 {
                        let i = (y + dy) * self.width + x + dx;
                        let (rgb, _, alpha) = self.graphic_pixel(i);
                        let source = self.color.yuv(rgb);
                        output[i] =
                            quantize(source[0] * alpha + f64::from(self.video[i]) * (1.0 - alpha));
                        for c in 0..2 {
                            chroma[c] += source[c + 1] * alpha + background[c + 1] * (1.0 - alpha);
                        }
                    }
                }
                output[pixels + uv] = quantize(chroma[0] / 4.0);
                output[pixels + pixels / 4 + uv] = quantize(chroma[1] / 4.0);
            }
        }

        output
    }

    pub fn encode_lookup(&self, value: f32) -> u8 {
        self.encode_lut[(value.clamp(0.0, 1.0) * 65535.0).round() as usize]
    }

    pub fn static_bytes(&self) -> usize {
        self.video.len()
            + self.graphic.len()
            + self.linear.len() * 4
            + self.inverse.len() * 4
            + self.rgba.source.len() * 2
            + self.rgba.alpha.len()
            + self.yuva444.source.len() * 2
            + self.yuva444.alpha.len()
            + self.yuva420.source.len() * 2
            + self.yuva420.alpha.len()
            + self.encode_lut.len()
            + self.decode_fine.len() * 4
            + self.encode_fine.len() * 4
            + self.linear_planar.len() * 4
    }
}

impl<'a> From<&'a Data> for super::linear_simd::Overlay<'a> {
    fn from(data: &'a Data) -> Self {
        Self {
            width: data.width,
            height: data.height,
            roi: data.roi,
            color: data.color,
            linear_planar: &data.linear_planar,
            decode_fine: &data.decode_fine,
            encode_fine: &data.encode_fine,
        }
    }
}
