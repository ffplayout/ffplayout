use std::{num::NonZeroU32, sync::Arc};

use anyhow::{Result, anyhow};
use softbuffer::{Context as SoftbufferContext, Surface};
use winit::window::Window;

use super::{
    DESKTOP_VOLUME_MAX,
    graphics::RgbaBitmap,
    render::{
        HELP_OVERLAY_PADDING, Rect, WindowFrame, WindowLogo, fit_rect, help_panel_rect, logo_rect,
        subtitle_rect,
    },
    video::VideoSurface,
};

pub(super) type WindowRenderer = SoftbufferRenderer;

pub(super) struct SoftbufferRenderer {
    surface: Surface<Arc<Window>, Arc<Window>>,
    _context: SoftbufferContext<Arc<Window>>,
    window: Arc<Window>,
    width: u32,
    height: u32,
}

#[cfg(feature = "desktop-cpu")]
impl SoftbufferRenderer {
    pub(super) fn new(window: Arc<Window>, _width: u32, _height: u32) -> Result<Self> {
        let context =
            SoftbufferContext::new(Arc::clone(&window)).map_err(|error| anyhow!("{error}"))?;
        let surface =
            Surface::new(&context, Arc::clone(&window)).map_err(|error| anyhow!("{error}"))?;
        Ok(Self {
            surface,
            _context: context,
            window,
            width: 0,
            height: 0,
        })
    }

    pub(super) fn resize_surface(&mut self, width: u32, height: u32) -> Result<()> {
        self.width = width;
        self.height = height;
        let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            return Ok(());
        };
        self.surface
            .resize(width, height)
            .map_err(|error| anyhow!("{error}"))
    }

    pub(super) fn resize_buffer(&mut self, _width: u32, _height: u32) -> Result<()> {
        Ok(())
    }

    pub(super) fn render(&mut self, frame: &WindowFrame, size: (u32, u32)) -> Result<()> {
        if self.width == 0 || self.height == 0 {
            return Ok(());
        }
        let mut target = self
            .surface
            .buffer_mut()
            .map_err(|error| anyhow!("{error}"))?;
        compose_window_frame(&mut target, self.width, self.height, frame, size)?;
        // Wayland uses this frame callback to throttle RedrawRequested events.
        // Without it, both softbuffer SHM buffers can be in flight and the
        // next buffer_mut call blocks waiting for the compositor.
        self.window.pre_present_notify();
        target.present().map_err(|error| anyhow!("{error}"))
    }
}

#[cfg(feature = "desktop-cpu")]
fn compose_window_frame(
    target: &mut [u32],
    width: u32,
    height: u32,
    frame: &WindowFrame,
    size: (u32, u32),
) -> Result<()> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    if let Some(video) = &frame.video {
        let rect = fit_rect(video.width, video.height, width, height);
        if rect.width != width || rect.height != height {
            target.fill(0);
        }
        scale_nearest(video, target, width, rect);
    } else {
        target.fill(0);
    }
    if let Some(logo) = &frame.logo {
        draw_logo(target, width, height, size, logo)?;
    }
    if let Some(subtitle) = &frame.subtitle {
        draw_subtitle(target, width, height, subtitle);
    }
    if frame.volume_overlay {
        draw_volume_overlay(target, width, height, frame.volume);
    }
    if let Some(help) = &frame.help {
        draw_help_overlay(target, width, height, help);
    }
    Ok(())
}

#[cfg(feature = "desktop-cpu")]
pub(super) fn scale_nearest(video: &VideoSurface, target: &mut [u32], stride: u32, dst: Rect) {
    if dst.width == 0 || dst.height == 0 {
        return;
    }
    if video.width == dst.width && video.height == dst.height {
        for row in 0..dst.height as usize {
            let source_start = row * video.width as usize;
            let target_start = (dst.y as usize + row) * stride as usize + dst.x as usize;
            target[target_start..target_start + dst.width as usize]
                .copy_from_slice(&video.pixels[source_start..source_start + video.width as usize]);
        }
        return;
    }
    for dy in 0..dst.height {
        let sy = (dy as u64 * video.height as u64 / dst.height as u64) as u32;
        for dx in 0..dst.width {
            let sx = (dx as u64 * video.width as u64 / dst.width as u64) as u32;
            let src = (sy * video.width + sx) as usize;
            let dest = ((dst.y + dy) * stride + dst.x + dx) as usize;
            target[dest] = video.pixels[src];
        }
    }
}

#[cfg(feature = "desktop-cpu")]
fn draw_logo(
    target: &mut [u32],
    width: u32,
    height: u32,
    window_size: (u32, u32),
    logo: &WindowLogo,
) -> Result<()> {
    let Some(rect) = logo_rect(logo, (window_size.0.max(1), window_size.1.max(1))) else {
        return Ok(());
    };
    let scale_x = f64::from(width) / f64::from(window_size.0.max(1));
    let scale_y = f64::from(height) / f64::from(window_size.1.max(1));
    let x = (f64::from(rect.x) * scale_x).round().max(0.0) as u32;
    let y = (f64::from(rect.y) * scale_y).round().max(0.0) as u32;
    let scaled_width = (f64::from(rect.width) * scale_x).round().max(1.0) as u32;
    let scaled_height = (f64::from(rect.height) * scale_y).round().max(1.0) as u32;
    draw_bitmap_scaled(
        target,
        width,
        height,
        &logo.bitmap,
        Rect {
            x,
            y,
            width: scaled_width,
            height: scaled_height,
        },
        logo.opacity,
    );
    Ok(())
}

#[cfg(feature = "desktop-cpu")]
fn draw_subtitle(target: &mut [u32], width: u32, height: u32, subtitle: &RgbaBitmap) {
    let rect = subtitle_rect(subtitle, (width, height));
    draw_bitmap_scaled(target, width, height, subtitle, rect, 255);
}

#[cfg(feature = "desktop-cpu")]
fn draw_bitmap_scaled(
    target: &mut [u32],
    target_width: u32,
    target_height: u32,
    bitmap: &RgbaBitmap,
    dst: Rect,
    opacity: u8,
) {
    if dst.width == 0 || dst.height == 0 {
        return;
    }
    let x_end = dst.x.saturating_add(dst.width).min(target_width);
    let y_end = dst.y.saturating_add(dst.height).min(target_height);
    for y in dst.y..y_end {
        let source_y = ((y - dst.y) as u64 * bitmap.height as u64 / dst.height as u64) as u32;
        for x in dst.x..x_end {
            let source_x = ((x - dst.x) as u64 * bitmap.width as u64 / dst.width as u64) as u32;
            let index = (source_y * bitmap.width + source_x) as usize * 4;
            let alpha = (u16::from(bitmap.pixels[index + 3]) * u16::from(opacity) / 255) as u8;
            if alpha > 0 {
                let color = (u32::from(bitmap.pixels[index]) << 16)
                    | (u32::from(bitmap.pixels[index + 1]) << 8)
                    | u32::from(bitmap.pixels[index + 2]);
                let target_index = (y * target_width + x) as usize;
                target[target_index] = blend(target[target_index], color, alpha);
            }
        }
    }
}

#[cfg(feature = "desktop-cpu")]
fn draw_volume_overlay(target: &mut [u32], width: u32, height: u32, volume: f64) {
    let panel_width = (width / 3).clamp(180, 360);
    let panel_height = 28;
    let margin = 24;
    let x = (width.saturating_sub(panel_width)) / 2;
    let y = height.saturating_sub(panel_height + margin);
    fill_blended_rect(
        target,
        width,
        height,
        Rect {
            x,
            y,
            width: panel_width,
            height: panel_height,
        },
        0x101214,
        220,
    );
    let track_margin = 8;
    let track_height = 6;
    let track_width = panel_width.saturating_sub(track_margin * 2);
    let track = Rect {
        x: x + track_margin,
        y: y + (panel_height - track_height) / 2,
        width: track_width,
        height: track_height,
    };
    fill_blended_rect(target, width, height, track, 0x4E5660, 255);
    let fill_width =
        ((volume / DESKTOP_VOLUME_MAX).clamp(0.0, 1.0) * f64::from(track_width)).round() as u32;
    fill_blended_rect(
        target,
        width,
        height,
        Rect {
            width: fill_width,
            ..track
        },
        0xEEEEEE,
        255,
    );
}

#[cfg(feature = "desktop-cpu")]
fn draw_help_overlay(target: &mut [u32], width: u32, height: u32, help: &RgbaBitmap) {
    let panel = help_panel_rect(help, (width, height));
    fill_blended_rect(target, width, height, panel, 0x101214, 205);

    let bitmap_width = help
        .width
        .min(panel.width.saturating_sub(HELP_OVERLAY_PADDING * 2));
    let bitmap_height = help
        .height
        .min(panel.height.saturating_sub(HELP_OVERLAY_PADDING * 2));
    draw_bitmap_scaled(
        target,
        width,
        height,
        help,
        Rect {
            x: panel.x + (panel.width.saturating_sub(bitmap_width)) / 2,
            y: panel.y + (panel.height.saturating_sub(bitmap_height)) / 2,
            width: bitmap_width,
            height: bitmap_height,
        },
        255,
    );
}

#[cfg(feature = "desktop-cpu")]
fn fill_blended_rect(
    target: &mut [u32],
    stride: u32,
    canvas_height: u32,
    rect: Rect,
    color: u32,
    alpha: u8,
) {
    let x_end = rect.x.saturating_add(rect.width).min(stride);
    let y_end = rect.y.saturating_add(rect.height).min(canvas_height);
    for y in rect.y..y_end {
        for x in rect.x..x_end {
            let index = (y * stride + x) as usize;
            target[index] = blend(target[index], color, alpha);
        }
    }
}

#[cfg(feature = "desktop-cpu")]
fn blend(dst: u32, src: u32, alpha: u8) -> u32 {
    let alpha = u32::from(alpha);
    let inverse = 255 - alpha;
    let channel = |shift| {
        (((src >> shift) & 0xff_u32) * alpha + ((dst >> shift) & 0xff_u32) * inverse) / 255_u32
    };
    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}
