use ffmpeg_next::frame;
use rayon::{ThreadPool, prelude::*};

use crate::{
    blend::{Plane, PlaneMut, blend_plane},
    ffmpeg::execution,
    preparation::ResourceLease,
};

pub struct OverlayFrame {
    pub lease: Option<ResourceLease>,
    pub frame: frame::Video, // YUVA420P
    pub chroma_alpha: Vec<u8>,
    /// Recompute after modifying the alpha plane. Prepared surfaces are immutable while used.
    pub bounds: OverlayBounds,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub opacity: u8,
}

#[derive(Clone, Copy)]
pub struct OverlayRef<'a> {
    pub frame: &'a frame::Video, // YUVA420P
    pub bounds: OverlayBounds,
    pub chroma_alpha: &'a [u8],
    pub chroma_alpha_stride: usize,
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
            bounds: self.bounds,
            chroma_alpha: &self.chroma_alpha,
            chroma_alpha_stride: self.width as usize / 2,
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            opacity: self.opacity,
        }
    }
}

/// Downsample a YUVA alpha plane once for the 4:2:0 chroma planes.
pub fn chroma_alpha(frame: &frame::Video, width: u32, height: u32) -> Vec<u8> {
    let width = width as usize;
    let height = height as usize;
    let alpha = frame.data(3);
    let alpha_stride = frame.stride(3);
    let chroma_width = width / 2;
    let chroma_height = height / 2;
    let mut downsampled = vec![0; chroma_width * chroma_height];

    for y in 0..chroma_height {
        for x in 0..chroma_width {
            let alpha_x = x * 2;
            let alpha_y = y * 2;
            let sum = u16::from(alpha[alpha_y * alpha_stride + alpha_x])
                + u16::from(alpha[alpha_y * alpha_stride + alpha_x + 1])
                + u16::from(alpha[(alpha_y + 1) * alpha_stride + alpha_x])
                + u16::from(alpha[(alpha_y + 1) * alpha_stride + alpha_x + 1]);
            downsampled[y * chroma_width + x] = (sum / 4) as u8;
        }
    }

    downsampled
}

/// Cached nontransparent source extent, rounded outwards to 2x2 chroma blocks.
/// This changes the blend region only, never the graphic's layout dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayBounds {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl OverlayBounds {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            width: width as usize & !1,
            height: height as usize & !1,
        }
    }
}

/// Scan once during preparation, not on the video processing path.
pub fn alpha_bounds(frame: &frame::Video) -> OverlayBounds {
    let (width, height) = (frame.width() as usize, frame.height() as usize);
    let stride = frame.stride(3);
    let alpha = frame.data(3);
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for y in 0..height {
        let row = &alpha[y * stride..y * stride + width];
        if let Some(first) = row.iter().position(|&a| a != 0) {
            let last = row.iter().rposition(|&a| a != 0).unwrap_or(first);
            left = left.min(first);
            top = top.min(y);
            right = right.max(last + 1);
            bottom = y + 1;
        }
    }
    if right == 0 {
        return OverlayBounds {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let x = left & !1;
    let y = top & !1;

    OverlayBounds {
        x,
        y,
        width: ((right + 1) & !1).min(width & !1) - x,
        height: ((bottom + 1) & !1).min(height & !1) - y,
    }
}

/// Explicit execution is useful for benchmarks; normal callers use Adaptive.
pub enum BlendExecution<'a> {
    Serial,
    Parallel(&'a ThreadPool),
    Adaptive,
}

// Selected from aggregate CPU and latency measurements; see graphics testing.
const PARALLEL_MIN_PIXELS: usize = 3_500_000;

fn use_parallel(width: usize, height: usize) -> bool {
    width.saturating_mul(height) >= PARALLEL_MIN_PIXELS && height >= 8
}

/// Warm the shared executor during resource preparation, away from frame blending.
pub(crate) fn prepare_execution(bounds: OverlayBounds) {
    if use_parallel(bounds.width, bounds.height) {
        let _ = execution::pool();
    }
}

/// Destination must be writable YUV420P; prepared sources are immutable YUVA420P.
/// Cached bounds retain layout dimensions and are shared by all execution modes.
pub fn blend_overlay(target: &mut frame::Video, overlay: OverlayRef<'_>, opacity_factor: f64) {
    blend_overlay_with_execution(target, overlay, opacity_factor, BlendExecution::Adaptive);
}

pub fn blend_overlay_with_execution(
    target: &mut frame::Video,
    overlay: OverlayRef<'_>,
    opacity_factor: f64,
    execution: BlendExecution<'_>,
) {
    if opacity_factor <= 0.0 {
        return;
    }
    let opacity = (f64::from(overlay.opacity) * opacity_factor.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8;
    if opacity == 0 {
        return;
    }
    let Some(clip) = ClipRect::new(target, overlay).and_then(|clip| clip.intersect(overlay.bounds))
    else {
        return;
    };
    // Busy channels fall back to serial work instead of queuing behind another
    // channel's large overlay. Explicit benchmark execution bypasses this gate.
    let permit = match execution {
        BlendExecution::Adaptive if use_parallel(clip.width, clip.height) => {
            execution::try_shared()
        }
        _ => None,
    };
    let pool = match execution {
        BlendExecution::Serial => None,
        BlendExecution::Parallel(pool) => Some(pool),
        BlendExecution::Adaptive => permit.as_ref().map(|permit| permit.pool),
    };
    if let Some(pool) = pool {
        pool.install(|| blend_clipped(target, overlay, &clip, opacity, Some(pool)));
    } else {
        blend_clipped(target, overlay, &clip, opacity, None);
    }
}

fn blend_clipped(
    target: &mut frame::Video,
    overlay: OverlayRef<'_>,
    clip: &ClipRect,
    opacity: u8,
    pool: Option<&ThreadPool>,
) {
    for plane in 0..3 {
        let divisor = if plane == 0 { 1 } else { 2 };
        let stride = target.stride(plane);
        let rows = clip.height / divisor;
        let first = clip.dst_y / divisor;
        let destination = &mut target.data_mut(plane)[first * stride..(first + rows) * stride];
        let (alpha, alpha_stride) = if plane == 0 {
            (overlay.frame.data(3), overlay.frame.stride(3))
        } else {
            (overlay.chroma_alpha, overlay.chroma_alpha_stride)
        };
        let run = |destination: &mut [u8], row: usize| {
            let height = destination.len() / stride;
            blend_plane(
                PlaneMut {
                    data: destination,
                    stride,
                    x: clip.dst_x / divisor,
                    y: 0,
                },
                Plane {
                    data: overlay.frame.data(plane),
                    stride: overlay.frame.stride(plane),
                    x: clip.src_x / divisor,
                    y: clip.src_y / divisor + row,
                },
                Plane {
                    data: alpha,
                    stride: alpha_stride,
                    x: clip.src_x / divisor,
                    y: clip.src_y / divisor + row,
                },
                clip.width / divisor,
                height,
                opacity,
            );
        };
        if let Some(pool) = pool {
            let band_rows = rows.div_ceil(pool.current_num_threads());
            destination
                .par_chunks_mut(band_rows * stride)
                .enumerate()
                .for_each(|(band, destination)| run(destination, band * band_rows));
        } else {
            run(destination, 0);
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
    fn intersect(mut self, bounds: OverlayBounds) -> Option<Self> {
        let left = self.src_x.max(bounds.x);
        let top = self.src_y.max(bounds.y);
        let right = (self.src_x + self.width).min(bounds.x + bounds.width);
        let bottom = (self.src_y + self.height).min(bounds.y + bounds.height);
        if left >= right || top >= bottom {
            return None;
        }
        self.dst_x += left - self.src_x;
        self.dst_y += top - self.src_y;
        self.src_x = left;
        self.src_y = top;
        self.width = right - left;
        self.height = bottom - top;

        Some(self)
    }

    fn new(target: &frame::Video, overlay: OverlayRef<'_>) -> Option<Self> {
        let target_width = i64::from(target.width());
        let target_height = i64::from(target.height());
        let overlay_width = i64::from(overlay.width);
        let overlay_height = i64::from(overlay.height);

        let dst_left = i64::from(overlay.x).max(0);
        let dst_top = i64::from(overlay.y).max(0);
        let dst_right = (i64::from(overlay.x) + overlay_width).min(target_width);
        let dst_bottom = (i64::from(overlay.y) + overlay_height).min(target_height);

        if dst_left >= dst_right || dst_top >= dst_bottom {
            return None;
        }

        let src_x = (dst_left - i64::from(overlay.x)).max(0) as usize;
        let src_y = (dst_top - i64::from(overlay.y)).max(0) as usize;
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
fn even_usize(value: usize) -> usize {
    value & !1
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::{frame, util::format::pixel::Pixel};

    use super::*;
    use crate::ffmpeg::execution::PoolPermit;
    use rayon::ThreadPoolBuilder;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn chroma_alpha_averages_each_luma_block() {
        let mut frame = frame::Video::new(Pixel::YUVA420P, 4, 2);
        let stride = frame.stride(3);
        let alpha = frame.data_mut(3);
        alpha[0] = 0;
        alpha[1] = 20;
        alpha[stride] = 40;
        alpha[stride + 1] = 60;
        alpha[2] = 80;
        alpha[3] = 100;
        alpha[stride + 2] = 120;
        alpha[stride + 3] = 140;

        assert_eq!(chroma_alpha(&frame, 4, 2), vec![30, 110]);
    }
    #[test]
    fn parallel_yuv_matches_production_with_clipping_strides_tails_and_opacity() {
        let pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();
        for width in [2, 18, 50] {
            for height in [2, 6, 18] {
                let mut source = frame::Video::new(Pixel::YUVA420P, width, height);
                for plane in 0..4 {
                    for (i, value) in source.data_mut(plane).iter_mut().enumerate() {
                        *value = ((i * 37 + plane * 51) % 256) as u8;
                    }
                }
                let mut overlay = OverlayFrame {
                    lease: None,
                    bounds: alpha_bounds(&source),
                    chroma_alpha: chroma_alpha(&source, width, height),
                    frame: source,
                    x: 0,
                    y: 0,
                    width,
                    height,
                    opacity: 255,
                };
                for x in [-3, 0, 8, 30] {
                    for opacity in [0, 127, 255] {
                        overlay.x = x;
                        overlay.y = 3;
                        overlay.opacity = opacity;
                        let mut expected = frame::Video::new(Pixel::YUV420P, 32, 24);
                        let mut actual = frame::Video::new(Pixel::YUV420P, 32, 24);
                        for plane in 0..3 {
                            expected.data_mut(plane).fill(70);
                            actual.data_mut(plane).fill(70);
                        }
                        blend_overlay(&mut expected, overlay.as_ref(), 1.0);
                        blend_overlay_with_execution(
                            &mut actual,
                            overlay.as_ref(),
                            1.0,
                            BlendExecution::Parallel(&pool),
                        );
                        for plane in 0..3 {
                            assert_eq!(
                                actual.data(plane),
                                expected.data(plane),
                                "{width}x{height}, x={x}, opacity={opacity}, plane={plane}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn cached_alpha_bounds_preserve_pixels_after_clipping_and_fading() {
        let mut source = frame::Video::new(Pixel::YUVA420P, 50, 20);
        for plane in 0..3 {
            source.data_mut(plane).fill(180);
        }
        source.data_mut(3).fill(0);
        let stride = source.stride(3);
        for y in 5..14 {
            for x in 3..42 {
                source.data_mut(3)[y * stride + x] = if (x + y) % 3 == 0 { 90 } else { 255 };
            }
        }
        let bounds = alpha_bounds(&source);
        assert_eq!(
            bounds,
            OverlayBounds {
                x: 2,
                y: 4,
                width: 40,
                height: 10
            }
        );
        let mut overlay = OverlayFrame {
            lease: None,
            bounds,
            chroma_alpha: chroma_alpha(&source, 50, 20),
            frame: source,
            x: 0,
            y: 0,
            width: 50,
            height: 20,
            opacity: 179,
        };
        let pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();
        for x in [-15, -3, 0, 7, 20, i32::MIN, i32::MAX] {
            for y in [-7, 0, 3, 21] {
                for factor in [0.0, 0.4, 1.0] {
                    overlay.x = x;
                    overlay.y = y;
                    let mut expected = frame::Video::new(Pixel::YUV420P, 32, 24);
                    for plane in 0..3 {
                        expected.data_mut(plane).fill(70);
                    }
                    let mut full = overlay.as_ref();
                    full.bounds = OverlayBounds::full(50, 20);
                    blend_overlay_with_execution(
                        &mut expected,
                        full,
                        factor,
                        BlendExecution::Serial,
                    );
                    for execution in [
                        BlendExecution::Serial,
                        BlendExecution::Parallel(&pool),
                        BlendExecution::Adaptive,
                    ] {
                        let mut actual = frame::Video::new(Pixel::YUV420P, 32, 24);
                        for plane in 0..3 {
                            actual.data_mut(plane).fill(70);
                        }
                        blend_overlay_with_execution(
                            &mut actual,
                            overlay.as_ref(),
                            factor,
                            execution,
                        );
                        for plane in 0..3 {
                            assert_eq!(
                                actual.data(plane),
                                expected.data(plane),
                                "x={x}, y={y}, factor={factor}, plane={plane}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_transparent_surface_has_empty_bounds_and_leaves_video_untouched() {
        let mut source = frame::Video::new(Pixel::YUVA420P, 8, 6);
        for plane in 0..4 {
            source.data_mut(plane).fill(0);
        }
        let bounds = alpha_bounds(&source);
        assert_eq!((bounds.width, bounds.height), (0, 0));
        let overlay = OverlayFrame {
            lease: None,
            bounds,
            chroma_alpha: chroma_alpha(&source, 8, 6),
            frame: source,
            x: 0,
            y: 0,
            width: 8,
            height: 6,
            opacity: 255,
        };
        let mut video = frame::Video::new(Pixel::YUV420P, 16, 12);
        for plane in 0..3 {
            video.data_mut(plane).fill(100);
        }
        blend_overlay(&mut video, overlay.as_ref(), 1.0);
        for plane in 0..3 {
            assert!(video.data(plane).iter().all(|&v| v == 100));
        }
    }

    #[test]
    fn adaptive_selection_is_conservative_and_busy_pool_permits_never_queue() {
        assert!(!use_parallel(1920, 1080));
        assert!(use_parallel(2560, 1440));
        let busy = AtomicBool::new(false);
        let first = PoolPermit::acquire(&busy).unwrap();
        assert!(PoolPermit::acquire(&busy).is_none());
        drop(first);
        assert!(PoolPermit::acquire(&busy).is_some());
    }

    #[test]
    fn a_large_adaptive_overlay_matches_serial_with_a_bounded_shared_executor() {
        let (width, height) = (2560, 1440);
        let mut source = frame::Video::new(Pixel::YUVA420P, width, height);
        for plane in 0..4 {
            source
                .data_mut(plane)
                .fill(if plane == 3 { 179 } else { 90 });
        }
        let bounds = alpha_bounds(&source);
        prepare_execution(bounds);
        if let Some(pool) = execution::pool() {
            assert!(pool.current_num_threads() <= 4);
        }
        let overlay = OverlayFrame {
            lease: None,
            bounds,
            chroma_alpha: chroma_alpha(&source, width, height),
            frame: source,
            x: 0,
            y: 0,
            width,
            height,
            opacity: 200,
        };
        let mut expected = frame::Video::new(Pixel::YUV420P, width, height);
        let mut actual = frame::Video::new(Pixel::YUV420P, width, height);
        for plane in 0..3 {
            expected.data_mut(plane).fill(70);
            actual.data_mut(plane).fill(70);
        }
        blend_overlay_with_execution(&mut expected, overlay.as_ref(), 0.7, BlendExecution::Serial);
        blend_overlay(&mut actual, overlay.as_ref(), 0.7);
        for plane in 0..3 {
            assert_eq!(actual.data(plane), expected.data(plane));
        }
    }
}
