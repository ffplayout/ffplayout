//! Shared desktop renderer data and geometry.

use crate::compositor::logo::logo_position;

use super::{graphics::RgbaBitmap, video::VideoSurface};

pub(super) const SUBTITLE_MARGIN_BOTTOM: u32 = 56;
pub(super) const HELP_OVERLAY_PADDING: u32 = 24;
pub(super) const HELP_OVERLAY_MARGIN: u32 = 8;

#[derive(Clone)]
pub(super) struct WindowFrame {
    pub(super) video: Option<VideoSurface>,
    pub(super) subtitle: Option<RgbaBitmap>,
    pub(super) logo: Option<WindowLogo>,
    pub(super) volume: f64,
    pub(super) volume_overlay: bool,
    pub(super) help: Option<RgbaBitmap>,
}

#[derive(Clone)]
pub(super) struct WindowLogo {
    pub(super) bitmap: RgbaBitmap,
    pub(super) position: String,
    pub(super) opacity: u8,
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

pub(super) fn logo_rect(logo: &WindowLogo, size: (u32, u32)) -> Option<Rect> {
    let (x, y) = logo_position(
        &logo.position,
        size.0,
        size.1,
        logo.bitmap.width,
        logo.bitmap.height,
    )
    .ok()?;
    Some(Rect {
        x,
        y,
        width: logo.bitmap.width,
        height: logo.bitmap.height,
    })
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

pub(super) fn help_panel_rect(bitmap: &RgbaBitmap, size: (u32, u32)) -> Rect {
    let width = bitmap
        .width
        .saturating_add(HELP_OVERLAY_PADDING * 2)
        .min(size.0);
    let height = bitmap
        .height
        .saturating_add(HELP_OVERLAY_PADDING * 2)
        .min(size.1);
    Rect {
        x: HELP_OVERLAY_MARGIN.min(size.0.saturating_sub(width)),
        y: HELP_OVERLAY_MARGIN.min(size.1.saturating_sub(height)),
        width,
        height,
    }
}
