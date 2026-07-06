use ffmpeg_next::frame;

use crate::compositor::blend::{Plane, PlaneMut, blend_plane};

pub struct OverlayFrame {
    pub frame: frame::Video, // YUVA420P
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: u8,
}

#[derive(Clone, Copy)]
pub struct OverlayRef<'a> {
    pub frame: &'a frame::Video, // YUVA420P
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: u8,
}

impl OverlayFrame {
    pub fn as_ref(&self) -> OverlayRef<'_> {
        OverlayRef {
            frame: &self.frame,
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            opacity: self.opacity,
        }
    }
}

pub fn blend_overlay(target: &mut frame::Video, overlay: OverlayRef<'_>, opacity_factor: f64) {
    // Expected formats:
    // target: YUV420P
    // overlay.frame: YUVA420P
    if opacity_factor <= 0.0 {
        return;
    }
    let opacity = ((overlay.opacity as f64) * opacity_factor.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8;
    if opacity == 0 {
        return;
    }

    let Some(clip) = ClipRect::new(target, overlay) else {
        return;
    };

    let overlay_y = overlay.frame.data(0);
    let overlay_u = overlay.frame.data(1);
    let overlay_v = overlay.frame.data(2);
    let overlay_a = overlay.frame.data(3);

    let overlay_y_stride = overlay.frame.stride(0);
    let overlay_u_stride = overlay.frame.stride(1);
    let overlay_v_stride = overlay.frame.stride(2);
    let overlay_a_stride = overlay.frame.stride(3);

    let target_y_stride = target.stride(0);
    let target_u_stride = target.stride(1);
    let target_v_stride = target.stride(2);

    let target_u_len = target.data(1).len();
    let target_v_len = target.data(2).len();

    blend_plane(
        PlaneMut {
            data: target.data_mut(0),
            stride: target_y_stride,
            x: clip.dst_x,
            y: clip.dst_y,
        },
        Plane {
            data: overlay_y,
            stride: overlay_y_stride,
            x: clip.src_x,
            y: clip.src_y,
        },
        Plane {
            data: overlay_a,
            stride: overlay_a_stride,
            x: clip.src_x,
            y: clip.src_y,
        },
        clip.width,
        clip.height,
        opacity,
    );

    let uv_src_x = clip.src_x / 2;
    let uv_src_y = clip.src_y / 2;
    let uv_dst_x = clip.dst_x / 2;
    let uv_dst_y = clip.dst_y / 2;
    let uv_width = clip.width / 2;
    let uv_height = clip.height / 2;

    for y in 0..uv_height {
        for x in 0..uv_width {
            let src_x = uv_src_x + x;
            let src_y = uv_src_y + y;
            let src_u_idx = src_y * overlay_u_stride + src_x;
            let src_v_idx = src_y * overlay_v_stride + src_x;

            if src_u_idx >= overlay_u.len() || src_v_idx >= overlay_v.len() {
                continue;
            }

            let ax = src_x * 2;
            let ay = src_y * 2;
            let alpha = avg_alpha_2x2(
                overlay_a,
                overlay_a_stride,
                ax,
                ay,
                overlay.width as usize,
                overlay.height as usize,
                opacity,
            );

            if alpha == 0 {
                continue;
            }

            let dst_x = uv_dst_x + x;
            let dst_y = uv_dst_y + y;
            let dst_u_idx = dst_y * target_u_stride + dst_x;
            let dst_v_idx = dst_y * target_v_stride + dst_x;

            if dst_u_idx >= target_u_len || dst_v_idx >= target_v_len {
                continue;
            }

            let dst_u = target.data(1)[dst_u_idx];
            let dst_v = target.data(2)[dst_v_idx];

            target.data_mut(1)[dst_u_idx] = blend_u8(dst_u, overlay_u[src_u_idx], alpha);
            target.data_mut(2)[dst_v_idx] = blend_u8(dst_v, overlay_v[src_v_idx], alpha);
        }
    }
}

struct ClipRect {
    src_x: usize,
    src_y: usize,
    dst_x: usize,
    dst_y: usize,
    width: usize,
    height: usize,
}

impl ClipRect {
    fn new(target: &frame::Video, overlay: OverlayRef<'_>) -> Option<Self> {
        let target_width = target.width() as i32;
        let target_height = target.height() as i32;
        let overlay_width = overlay.width as i32;
        let overlay_height = overlay.height as i32;

        let dst_left = overlay.x.max(0);
        let dst_top = overlay.y.max(0);
        let dst_right = (overlay.x + overlay_width).min(target_width);
        let dst_bottom = (overlay.y + overlay_height).min(target_height);

        if dst_left >= dst_right || dst_top >= dst_bottom {
            return None;
        }

        let src_x = (dst_left - overlay.x).max(0) as usize;
        let src_y = (dst_top - overlay.y).max(0) as usize;
        let width = (dst_right - dst_left) as usize;
        let height = (dst_bottom - dst_top) as usize;

        Some(Self {
            src_x: even_usize(src_x),
            src_y: even_usize(src_y),
            dst_x: even_usize(dst_left as usize),
            dst_y: even_usize(dst_top as usize),
            width: even_usize(width),
            height: even_usize(height),
        })
        .filter(|clip| clip.width > 0 && clip.height > 0)
    }
}

#[inline]
fn blend_u8(dst: u8, src: u8, alpha: u8) -> u8 {
    let a = alpha as u16;
    (((dst as u16 * (255 - a)) + (src as u16 * a) + 127) / 255) as u8
}

#[inline]
fn mul_alpha(src_alpha: u8, opacity: u8) -> u8 {
    ((src_alpha as u16 * opacity as u16 + 127) / 255) as u8
}

fn avg_alpha_2x2(
    alpha_plane: &[u8],
    alpha_stride: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    opacity: u8,
) -> u8 {
    let mut sum = 0_u16;
    let mut count = 0_u16;

    for dy in 0..2 {
        for dx in 0..2 {
            let px = x + dx;
            let py = y + dy;

            if px >= width || py >= height {
                continue;
            }

            let idx = py * alpha_stride + px;
            if idx >= alpha_plane.len() {
                continue;
            }

            sum += alpha_plane[idx] as u16;
            count += 1;
        }
    }

    if count == 0 {
        return 0;
    }

    let avg = (sum / count) as u8;
    mul_alpha(avg, opacity)
}

#[inline]
fn even_usize(value: usize) -> usize {
    value & !1
}
