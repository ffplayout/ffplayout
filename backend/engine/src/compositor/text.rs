use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Mutex, OnceLock, PoisonError},
};

use anyhow::{Context, Result, anyhow};
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use ffmpeg_next::{frame, util::format::pixel::Pixel};
use regex::Regex;

use crate::{
    compositor::overlay::{OverlayFrame, blend_overlay},
    utils::{
        config::{RgbaColor, TextConfig, TextPosition, TextScroll, TextWeight},
        helper::even,
    },
};

static TEXT_RENDERER: OnceLock<Mutex<TextRenderer>> = OnceLock::new();

struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl TextRenderer {
    fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }
}

pub(crate) fn init() {
    TEXT_RENDERER.get_or_init(|| Mutex::new(TextRenderer::new()));
}

fn renderer() -> &'static Mutex<TextRenderer> {
    TEXT_RENDERER.get_or_init(|| Mutex::new(TextRenderer::new()))
}

pub(crate) fn available_font_families() -> Vec<String> {
    let renderer = renderer().lock().unwrap_or_else(PoisonError::into_inner);
    renderer
        .font_system
        .db()
        .faces()
        .filter_map(|face| face.families.first().map(|(family, _)| family.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub struct TextOverlay {
    overlay: OverlayFrame,
    scroll: TextScroll,
    scroll_repeat: i32,
    fade_in_frames: i64,
    fade_out_frames: i64,
    fade_start_pts: i64,
    scroll_start_pts: i64,
    end_pts: Option<i64>,
    base_x: i32,
    base_y: i32,
    output_width: u32,
    fps: u32,
}

impl TextOverlay {
    #[allow(clippy::too_many_arguments)]
    pub fn load(
        config: &TextConfig,
        media_path: &str,
        output_width: u32,
        output_height: u32,
        fps: u32,
        fade_start_pts: i64,
        scroll_start_pts: i64,
        end_pts: Option<i64>,
    ) -> Result<Option<Self>> {
        let Some(text) = overlay_text(config, media_path) else {
            return Ok(None);
        };
        if text.trim().is_empty() {
            return Ok(None);
        }

        let overlay = render_text_overlay(config, &text, output_width, output_height)?;
        let base_x = text_position(config.position_x, output_width, overlay.width);
        let base_y = text_position(config.position_y, output_height, overlay.height);

        Ok(Some(Self {
            overlay: OverlayFrame {
                frame: overlay.frame,
                x: base_x,
                y: base_y,
                width: overlay.width,
                height: overlay.height,
                opacity: (config.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
            },
            scroll: config.scroll,
            scroll_repeat: config.scroll_repeat,
            fade_in_frames: seconds_to_frames(config.fade_in_seconds, fps),
            fade_out_frames: seconds_to_frames(config.fade_out_seconds, fps),
            fade_start_pts,
            scroll_start_pts,
            end_pts,
            base_x,
            base_y,
            output_width,
            fps,
        }))
    }

    pub fn blend(&mut self, target: &mut frame::Video, pts: i64, scroll_pts: i64) {
        let opacity = self.opacity_at(pts);
        if opacity <= 0.0 {
            return;
        }

        let x = self.x_at(scroll_pts);
        self.overlay.x = even_signed(x);
        self.overlay.y = even_signed(self.base_y);
        blend_overlay(target, self.overlay.as_ref(), opacity);
    }

    fn opacity_at(&self, pts: i64) -> f64 {
        let mut opacity: f64 = 1.0;
        if self.fade_in_frames > 0 {
            let elapsed = (pts - self.fade_start_pts).max(0);
            opacity = opacity.min(elapsed as f64 / self.fade_in_frames as f64);
        }
        if self.fade_out_frames > 0
            && let Some(end_pts) = self.end_pts
        {
            let remaining = (end_pts - pts).max(0);
            opacity = opacity.min(remaining as f64 / self.fade_out_frames as f64);
        }
        opacity.clamp(0.0, 1.0)
    }

    fn x_at(&self, pts: i64) -> i32 {
        let elapsed = (pts - self.scroll_start_pts).max(0);
        match self.scroll {
            TextScroll::None => self.base_x,
            TextScroll::LeftToRight { pixels_per_second } => {
                let offset = self.scroll_offset(elapsed, pixels_per_second);
                -i32::try_from(self.overlay.width).unwrap_or(i32::MAX)
                    + i32::try_from(offset).unwrap_or(i32::MAX)
            }
            TextScroll::RightToLeft { pixels_per_second } => {
                let offset = self.scroll_offset(elapsed, pixels_per_second);
                i32::try_from(self.output_width).unwrap_or(i32::MAX)
                    - i32::try_from(offset).unwrap_or(i32::MAX)
            }
        }
    }

    fn scroll_offset(&self, elapsed: i64, pixels_per_second: u32) -> i64 {
        let offset = elapsed * i64::from(pixels_per_second) / i64::from(self.fps.max(1));
        let travel = i64::from(self.output_width) + i64::from(self.overlay.width);
        if travel <= 0 {
            return offset;
        }

        if self.scroll_repeat < 0 {
            return offset % travel;
        }

        let cycles = i64::from(self.scroll_repeat) + 1;
        if offset < travel.saturating_mul(cycles) {
            offset % travel
        } else {
            offset
        }
    }
}

struct RenderedText {
    frame: frame::Video,
    width: u32,
    height: u32,
}

fn overlay_text(config: &TextConfig, media_path: &str) -> Option<String> {
    if config.use_filename {
        if let Some(regex) = &config.filename_regex
            && let Ok(regex) = Regex::new(regex)
            && let Some(captures) = regex.captures(media_path)
            && let Some(capture) = captures.get(1)
        {
            return Some(capture.as_str().to_string());
        }

        return Path::new(media_path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned);
    }

    config.text.clone()
}

fn render_text_overlay(
    config: &TextConfig,
    text: &str,
    output_width: u32,
    output_height: u32,
) -> Result<RenderedText> {
    if !config.font_size.is_finite() || config.font_size <= 0.0 {
        return Err(anyhow!("text font size must be a positive number"));
    }

    let padding = config
        .background
        .map(|background| background.padding)
        .unwrap_or(0);
    let render_width = render_width(config, text, output_width, padding);
    let text_width = render_width
        .saturating_sub(padding.saturating_mul(2))
        .max(2);
    let text_height = output_height
        .saturating_sub(padding.saturating_mul(2))
        .max(2);
    let line_height = (config.font_size + config.line_spacing.max(0.0))
        .ceil()
        .max(config.font_size);

    let mut rgba = vec![0_u8; (render_width as usize) * (output_height as usize) * 4];
    let mut renderer = renderer().lock().unwrap_or_else(PoisonError::into_inner);
    let TextRenderer {
        font_system,
        swash_cache,
    } = &mut *renderer;
    let metrics = Metrics::new(config.font_size, line_height);
    let mut buffer = Buffer::new(font_system, metrics);
    buffer.set_size(Some(text_width as f32), Some(text_height as f32));

    let mut attrs = Attrs::new();
    if let Some(family) = &config.font_family {
        attrs = attrs.family(Family::Name(family));
    }
    attrs = attrs.weight(match config.font_weight {
        TextWeight::Normal => Weight::NORMAL,
        TextWeight::Semibold => Weight::SEMIBOLD,
        TextWeight::Bold => Weight::BOLD,
    });
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    let text_color = Color::rgba(
        config.text_color.r,
        config.text_color.g,
        config.text_color.b,
        config.text_color.a,
    );
    let mut bounds = Bounds::default();
    buffer.draw(
        font_system,
        swash_cache,
        text_color,
        |x, y, width, height, color| {
            let color = rgba_color(color);
            let Some(dest) = PixelRect::new(
                x + i32::try_from(padding).unwrap_or(0),
                y + i32::try_from(padding).unwrap_or(0),
                width,
                height,
                render_width,
                output_height,
            ) else {
                return;
            };
            bounds.include(dest.x, dest.y, dest.width, dest.height);
            for py in 0..dest.height {
                for px in 0..dest.width {
                    let idx = ((dest.y + py) * render_width as usize + dest.x + px) * 4;
                    alpha_composite(&mut rgba[idx..idx + 4], color);
                }
            }
        },
    );

    let Some(mut bounds) = bounds.finish() else {
        return Err(anyhow!("text overlay produced no visible pixels"));
    };
    bounds.expand(
        padding as usize,
        render_width as usize,
        output_height as usize,
    );

    if let Some(background) = config.background {
        let mut boxed = vec![0_u8; rgba.len()];
        draw_box(&mut boxed, render_width, &bounds, background.color);
        for y in bounds.y..bounds.y + bounds.height {
            for x in bounds.x..bounds.x + bounds.width {
                let idx = (y * render_width as usize + x) * 4;
                let text = RgbaColor {
                    r: rgba[idx],
                    g: rgba[idx + 1],
                    b: rgba[idx + 2],
                    a: rgba[idx + 3],
                };
                alpha_composite(&mut boxed[idx..idx + 4], text);
            }
        }
        rgba = boxed;
    }

    let width = even(bounds.width as u32).max(2);
    let height = even(bounds.height as u32).max(2);
    let mut cropped = vec![0_u8; width as usize * height as usize * 4];
    for y in 0..height as usize {
        for x in 0..width as usize {
            let src_idx = ((bounds.y + y) * render_width as usize + bounds.x + x) * 4;
            let dst_idx = (y * width as usize + x) * 4;
            cropped[dst_idx..dst_idx + 4].copy_from_slice(&rgba[src_idx..src_idx + 4]);
        }
    }

    Ok(RenderedText {
        frame: rgba_to_yuva420p(&cropped, width, height)
            .context("failed to create text overlay frame")?,
        width,
        height,
    })
}

fn render_width(config: &TextConfig, text: &str, output_width: u32, padding: u32) -> u32 {
    if matches!(config.scroll, TextScroll::None) {
        return output_width.max(2);
    }

    let estimated_text_width = (text.chars().count() as f32 * config.font_size * 0.75).ceil();
    let estimated = estimated_text_width as u32 + padding.saturating_mul(2);
    let max_width = output_width.saturating_mul(8).max(output_width);
    even(estimated.max(output_width).min(max_width)).max(2)
}

fn rgba_to_yuva420p(rgba: &[u8], width: u32, height: u32) -> Result<frame::Video> {
    let mut frame = frame::Video::new(Pixel::YUVA420P, width, height);
    let width = width as usize;
    let height = height as usize;

    let y_stride = frame.stride(0);
    let u_stride = frame.stride(1);
    let v_stride = frame.stride(2);
    let a_stride = frame.stride(3);

    for y in 0..height {
        for x in 0..width {
            let src_idx = (y * width + x) * 4;
            let (yy, _, _) = rgb_to_yuv(rgba[src_idx], rgba[src_idx + 1], rgba[src_idx + 2]);
            frame.data_mut(0)[y * y_stride + x] = yy;
            frame.data_mut(3)[y * a_stride + x] = rgba[src_idx + 3];
        }
    }

    for y in 0..height / 2 {
        for x in 0..width / 2 {
            let mut r = 0_u16;
            let mut g = 0_u16;
            let mut b = 0_u16;
            let mut count = 0_u16;
            for dy in 0..2 {
                for dx in 0..2 {
                    let px = x * 2 + dx;
                    let py = y * 2 + dy;
                    let src_idx = (py * width + px) * 4;
                    r += rgba[src_idx] as u16;
                    g += rgba[src_idx + 1] as u16;
                    b += rgba[src_idx + 2] as u16;
                    count += 1;
                }
            }
            let (_, u, v) = rgb_to_yuv((r / count) as u8, (g / count) as u8, (b / count) as u8);
            frame.data_mut(1)[y * u_stride + x] = u;
            frame.data_mut(2)[y * v_stride + x] = v;
        }
    }

    Ok(frame)
}

fn rgb_to_yuv(r: u8, g: u8, b: u8) -> (u8, u8, u8) {
    let r = i32::from(r);
    let g = i32::from(g);
    let b = i32::from(b);
    let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
    let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
    let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
    (clamp_u8(y), clamp_u8(u), clamp_u8(v))
}

fn text_position(position: TextPosition, output: u32, overlay: u32) -> i32 {
    match position {
        TextPosition::Pixels(value) => value,
        TextPosition::Center => ((output as i32 - overlay as i32) / 2).max(0),
        TextPosition::End(offset) => output as i32 - overlay as i32 - offset,
    }
}

fn seconds_to_frames(seconds: f64, fps: u32) -> i64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        0
    } else {
        (seconds * f64::from(fps)).round().max(1.0) as i64
    }
}

fn rgba_color(color: Color) -> RgbaColor {
    let [r, g, b, a] = color.as_rgba();
    RgbaColor { r, g, b, a }
}

fn alpha_composite(dst: &mut [u8], src: RgbaColor) {
    let alpha = src.a as u16;
    let inv_alpha = 255 - alpha;
    dst[0] = (((u16::from(dst[0]) * inv_alpha) + (u16::from(src.r) * alpha) + 127) / 255) as u8;
    dst[1] = (((u16::from(dst[1]) * inv_alpha) + (u16::from(src.g) * alpha) + 127) / 255) as u8;
    dst[2] = (((u16::from(dst[2]) * inv_alpha) + (u16::from(src.b) * alpha) + 127) / 255) as u8;
    dst[3] = (u16::from(dst[3]) + alpha - ((u16::from(dst[3]) * alpha + 127) / 255)) as u8;
}

fn draw_box(rgba: &mut [u8], width: u32, bounds: &ResolvedBounds, color: RgbaColor) {
    let width = width as usize;
    for y in bounds.y..bounds.y + bounds.height {
        for x in bounds.x..bounds.x + bounds.width {
            let idx = (y * width + x) * 4;
            alpha_composite(&mut rgba[idx..idx + 4], color);
        }
    }
}

#[inline]
fn clamp_u8(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

#[inline]
fn even_signed(value: i32) -> i32 {
    value & !1
}

struct PixelRect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl PixelRect {
    fn new(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        max_width: u32,
        max_height: u32,
    ) -> Option<Self> {
        let left = x.max(0);
        let top = y.max(0);
        let right = (x + width as i32).min(max_width as i32);
        let bottom = (y + height as i32).min(max_height as i32);

        if left >= right || top >= bottom {
            return None;
        }

        Some(Self {
            x: left as usize,
            y: top as usize,
            width: (right - left) as usize,
            height: (bottom - top) as usize,
        })
    }
}

#[derive(Default)]
struct Bounds {
    x_min: Option<usize>,
    y_min: Option<usize>,
    x_max: usize,
    y_max: usize,
}

impl Bounds {
    fn include(&mut self, x: usize, y: usize, width: usize, height: usize) {
        self.x_min = Some(self.x_min.map_or(x, |min| min.min(x)));
        self.y_min = Some(self.y_min.map_or(y, |min| min.min(y)));
        self.x_max = self.x_max.max(x + width);
        self.y_max = self.y_max.max(y + height);
    }

    fn finish(self) -> Option<ResolvedBounds> {
        let x = self.x_min?;
        let y = self.y_min?;
        Some(ResolvedBounds {
            x,
            y,
            width: self.x_max.saturating_sub(x).max(1),
            height: self.y_max.saturating_sub(y).max(1),
        })
    }
}

struct ResolvedBounds {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl ResolvedBounds {
    fn expand(&mut self, padding: usize, max_width: usize, max_height: usize) {
        let x = self.x.saturating_sub(padding);
        let y = self.y.saturating_sub(padding);
        let right = (self.x + self.width + padding).min(max_width);
        let bottom = (self.y + self.height + padding).min(max_height);
        self.x = x;
        self.y = y;
        self.width = right.saturating_sub(x).max(1);
        self.height = bottom.saturating_sub(y).max(1);
    }
}
