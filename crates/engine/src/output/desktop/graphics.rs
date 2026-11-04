use std::sync::Arc;

use anyhow::Result;
use ff_graphics::bitmap::{StraightRgbaBitmap, source_over_straight};

use crate::{
    graphics::text_adapter::{render_left_aligned_text_bitmap, render_wrapped_text_bitmap},
    utils::config::{RgbaColor, TextWeight},
};

pub(super) const SUBTITLE_FONT_SIZE: f32 = 24.0;
pub(super) const SUBTITLE_FULLSCREEN_FONT_SIZE: f32 = 44.0;
pub(super) const SUBTITLE_OUTLINE: u32 = 2;
pub(super) const SUBTITLE_MAX_WIDTH_PERCENT: u32 = 92;

/// Straight-alpha RGBA8 consumed by both desktop rendering backends.
#[derive(Clone)]
pub(super) struct RgbaBitmap {
    pub(super) pixels: Arc<[u8]>,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// Help pixels and the DPI scale used to rasterize them travel together.
#[derive(Clone)]
pub(super) struct HelpOverlay {
    pub(super) bitmap: RgbaBitmap,
    pub(super) scale_factor: f64,
}

pub(super) fn create_subtitle_bitmap(
    text: &str,
    window_width: u32,
    large_display: bool,
) -> Result<Option<RgbaBitmap>> {
    let max_width = (window_width * SUBTITLE_MAX_WIDTH_PERCENT / 100).max(1);
    let font_size = subtitle_font_size(large_display);
    let text_width = max_width.saturating_sub(SUBTITLE_OUTLINE * 2).max(1);
    let white = render_wrapped_text_bitmap(
        text,
        font_size,
        TextWeight::Semibold,
        RgbaColor {
            r: 248,
            g: 248,
            b: 248,
            a: 255,
        },
        text_width,
    )?;
    let black = render_wrapped_text_bitmap(
        text,
        font_size,
        TextWeight::Semibold,
        RgbaColor {
            r: 0,
            g: 0,
            b: 0,
            a: 230,
        },
        text_width,
    )?;
    let width = white.width + SUBTITLE_OUTLINE * 2;
    let height = white.height + SUBTITLE_OUTLINE * 2;
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    let outline = SUBTITLE_OUTLINE as i32;

    for y in -outline..=outline {
        for x in -outline..=outline {
            if x == 0 && y == 0 {
                continue;
            }

            let distance = ((x * x + y * y) as f32).sqrt();

            if distance <= outline as f32 + 0.25 {
                let alpha = (1.0 - distance / (outline as f32 + 0.75)).clamp(0.25, 1.0);
                composite_bitmap(&mut pixels, width, &black, outline + x, outline + y, alpha);
            }
        }
    }
    composite_bitmap(&mut pixels, width, &white, outline, outline, 1.0);

    Ok(Some(RgbaBitmap {
        pixels: pixels.into(),
        width,
        height,
    }))
}

pub(super) fn create_help_bitmap(
    window_width: u32,
    large_display: bool,
    scale_factor: f64,
) -> Result<Option<HelpOverlay>> {
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let font_size = if large_display { 28.0 } else { 18.0 } * scale_factor as f32;
    let logical_width = f64::from(window_width) / scale_factor;
    let text_width = ((logical_width * 0.6).clamp(280.0, 680.0) * scale_factor)
        .round()
        .max(1.0) as u32;
    let bitmap = render_left_aligned_text_bitmap(
        "Keyboard shortcuts\n\nE  Previous clip\nR  Reset playlist\nT  Next clip\n\nF  Fullscreen\nEsc  Stop playout\nLeft/Right  Volume\nS  Toggle subtitles\nH  Close help",
        font_size,
        TextWeight::Semibold,
        RgbaColor {
            r: 248,
            g: 248,
            b: 248,
            a: 255,
        },
        text_width,
    )?;

    Ok(Some(HelpOverlay {
        bitmap: RgbaBitmap {
            pixels: bitmap.pixels.into(),
            width: bitmap.width,
            height: bitmap.height,
        },
        scale_factor,
    }))
}

pub(super) fn subtitle_font_size(large_display: bool) -> f32 {
    if large_display {
        SUBTITLE_FULLSCREEN_FONT_SIZE
    } else {
        SUBTITLE_FONT_SIZE
    }
}

fn composite_bitmap(
    destination: &mut [u8],
    destination_width: u32,
    source: &StraightRgbaBitmap,
    x: i32,
    y: i32,
    alpha_scale: f32,
) {
    for source_y in 0..source.height as usize {
        let destination_y = y + source_y as i32;

        if destination_y < 0 {
            continue;
        }

        for source_x in 0..source.width as usize {
            let destination_x = x + source_x as i32;

            if destination_x < 0 {
                continue;
            }

            let destination_index =
                (destination_y as usize * destination_width as usize + destination_x as usize) * 4;
            let source_index = (source_y * source.width as usize + source_x) * 4;

            if destination_index + 4 > destination.len() {
                continue;
            }

            let pixel = [
                source.pixels[source_index],
                source.pixels[source_index + 1],
                source.pixels[source_index + 2],
                (f32::from(source.pixels[source_index + 3]) * alpha_scale).round() as u8,
            ];
            alpha_composite_rgba(
                &mut destination[destination_index..destination_index + 4],
                &pixel,
            );
        }
    }
}

fn alpha_composite_rgba(destination: &mut [u8], source: &[u8]) {
    let pixel = destination
        .try_into()
        .expect("RGBA pixel has four components");
    let source = source.try_into().expect("RGBA pixel has four components");
    source_over_straight(pixel, source);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_is_rasterized_at_monitor_resolution_instead_of_upscaling_low_dpi_pixels() {
        for large in [false, true] {
            let baseline = create_help_bitmap(1200, large, 1.0).unwrap().unwrap();

            for scale in [1.25, 1.5, 2.0] {
                let help = create_help_bitmap((1200.0 * scale) as u32, large, scale)
                    .unwrap()
                    .unwrap();
                assert_eq!(help.scale_factor, scale);
                assert!(
                    (f64::from(help.bitmap.width) - f64::from(baseline.bitmap.width) * scale).abs()
                        <= 4.0
                );
                assert!(
                    (f64::from(help.bitmap.height) - f64::from(baseline.bitmap.height) * scale)
                        .abs()
                        <= 4.0
                );
                assert_eq!(
                    help.bitmap.pixels.len(),
                    help.bitmap.width as usize * help.bitmap.height as usize * 4
                );
                assert!(
                    help.bitmap
                        .pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel[3] != 0)
                );
            }
        }
    }
}
