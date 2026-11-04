//! Shared desktop renderer data and geometry.

use super::{
    graphics::{HelpOverlay, RgbaBitmap},
    video::VideoSurface,
};

pub(super) const SUBTITLE_MARGIN_BOTTOM: u32 = 56;
const HELP_OVERLAY_PADDING: u32 = 24;
const HELP_OVERLAY_MARGIN: u32 = 8;

#[derive(Clone)]
pub(super) struct WindowFrame {
    pub(super) video: Option<VideoSurface>,
    pub(super) subtitle: Option<RgbaBitmap>,
    pub(super) volume: f64,
    pub(super) volume_overlay: bool,
    pub(super) help: Option<HelpOverlay>,
}

impl Default for WindowFrame {
    fn default() -> Self {
        Self {
            video: None,
            subtitle: None,
            volume: 1.0,
            volume_overlay: false,
            help: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Rect {
    pub(super) x: u32,
    pub(super) y: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) fn fit_rect(src_width: u32, src_height: u32, dst_width: u32, dst_height: u32) -> Rect {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }

    let scale = (dst_width as f64 / src_width as f64).min(dst_height as f64 / src_height as f64);
    let width = (src_width as f64 * scale).round().max(1.0) as u32;
    let height = (src_height as f64 * scale).round().max(1.0) as u32;
    Rect {
        x: (dst_width - width) / 2,
        y: (dst_height - height) / 2,
        width,
        height,
    }
}

pub(super) fn subtitle_rect(bitmap: &RgbaBitmap, size: (u32, u32)) -> Rect {
    Rect {
        x: (size.0.saturating_sub(bitmap.width)) / 2,
        y: size
            .1
            .saturating_sub(bitmap.height + SUBTITLE_MARGIN_BOTTOM),
        width: bitmap.width,
        height: bitmap.height,
    }
}

pub(super) struct HelpLayout {
    pub(super) panel: Rect,
    pub(super) text: Rect,
}

/// Fit the complete help image, with DPI-scaled padding, identically on CPU/GPU.
pub(super) fn help_layout(help: &HelpOverlay, size: (u32, u32)) -> HelpLayout {
    let margin = (f64::from(HELP_OVERLAY_MARGIN) * help.scale_factor).round() as u32;
    // Keep decoration from consuming a tiny window: reserve most of each
    // available dimension for text before fitting the bitmap.
    let margin = margin.min(size.0 / 8).min(size.1 / 8);
    let available = (size.0 - margin * 2, size.1 - margin * 2);
    let padding = (f64::from(HELP_OVERLAY_PADDING) * help.scale_factor).round() as u32;
    let padding = padding.min(available.0 / 8).min(available.1 / 8);
    let fitted = fit_rect(
        help.bitmap.width,
        help.bitmap.height,
        available.0 - padding * 2,
        available.1 - padding * 2,
    );
    let width = fitted.width.min(help.bitmap.width);
    let height = fitted.height.min(help.bitmap.height);

    HelpLayout {
        panel: Rect {
            x: margin,
            y: margin,
            width: width + padding * 2,
            height: height + padding * 2,
        },
        text: Rect {
            x: margin + padding,
            y: margin + padding,
            width,
            height,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help(width: u32, height: u32, scale_factor: f64) -> HelpOverlay {
        HelpOverlay {
            bitmap: RgbaBitmap {
                pixels: Vec::new().into(),
                width,
                height,
            },
            scale_factor,
        }
    }

    #[test]
    fn help_layout_preserves_logical_dimensions_at_integer_and_fractional_dpi() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let bitmap = help((400.0 * scale) as u32, (300.0 * scale) as u32, scale);
            let layout = help_layout(&bitmap, ((1000.0 * scale) as u32, (800.0 * scale) as u32));

            assert_eq!(layout.panel.x, (8.0 * scale).round() as u32);
            assert_eq!(layout.panel.y, (8.0 * scale).round() as u32);
            assert_eq!(layout.text.width, bitmap.bitmap.width);
            assert_eq!(layout.text.height, bitmap.bitmap.height);
            assert_eq!(
                layout.text.x - layout.panel.x,
                (24.0 * scale).round() as u32
            );
            assert_eq!(
                layout.panel.width,
                bitmap.bitmap.width + 2 * (24.0 * scale).round() as u32
            );
        }
    }

    #[test]
    fn help_layout_fits_the_entire_image_without_cropping_or_changing_aspect() {
        let bitmap = help(800, 600, 2.0);

        for size in [
            (0, 0),
            (0, 80),
            (80, 0),
            (1, 1),
            (1, 80),
            (80, 1),
            (100, 80),
            (200, 128),
            (300, 200),
            (400, 800),
            (1600, 1200),
        ] {
            let layout = help_layout(&bitmap, size);
            assert!(layout.panel.x + layout.panel.width <= size.0);
            assert!(layout.panel.y + layout.panel.height <= size.1);
            assert!(layout.text.x + layout.text.width <= layout.panel.x + layout.panel.width);
            assert!(layout.text.y + layout.text.height <= layout.panel.y + layout.panel.height);
            assert!(layout.text.width <= 800 && layout.text.height <= 600);

            if size.0 > 0 && size.1 > 0 {
                assert!(
                    layout.text.width > 0 && layout.text.height > 0,
                    "empty help in {size:?}"
                );
                assert!(
                    (i64::from(layout.text.width) * 600 - i64::from(layout.text.height) * 800)
                        .abs()
                        <= 1400
                );
            } else {
                assert_eq!((layout.text.width, layout.text.height), (0, 0));
            }
        }
    }
}
