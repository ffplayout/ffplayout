//! Shared immutable CPU surfaces and per-resolution scaling caches.
use std::sync::Arc;

use anyhow::{Result, ensure};
use ffmpeg_next::{format::Pixel, frame};

use super::overlay::{OverlayFrame, alpha_bounds, blend_overlay, chroma_alpha};

/// Immutable pixels and per-program-frame placement; safe to retain in bounded queues.
#[derive(Clone)]
pub struct SurfaceFrame {
    pub(super) surface: Arc<OverlayFrame>,
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) opacity: f64,
}

pub struct SurfaceScaleCache {
    geometry: Option<((u32, u32), (u32, u32))>,
    source: Option<Arc<OverlayFrame>>,
    scaled: Option<Arc<OverlayFrame>>,
    budget: usize,
    max_dimension: u32,
}

impl Default for SurfaceScaleCache {
    fn default() -> Self {
        Self {
            geometry: None,
            source: None,
            scaled: None,
            budget: usize::MAX,
            max_dimension: i32::MAX as u32,
        }
    }
}

impl SurfaceScaleCache {
    pub(super) fn storage_bytes(&self) -> usize {
        self.scaled.as_ref().map_or(0, |surface| {
            (0..surface.frame.planes())
                .map(|plane| surface.frame.data(plane).len())
                .sum::<usize>()
                + surface.chroma_alpha.len()
        })
    }

    pub(super) fn set_limits(&mut self, bytes: usize, max_dimension: u32) {
        self.budget = bytes;
        self.max_dimension = max_dimension;
    }
}

impl SurfaceFrame {
    pub fn dimensions(&self) -> (u32, u32) {
        (self.surface.width, self.surface.height)
    }

    pub fn blend(&self, target: &mut frame::Video) {
        let mut overlay = self.surface.as_ref().as_ref();
        overlay.x = self.x;
        overlay.y = self.y;
        blend_overlay(target, overlay, self.opacity);
    }

    /// Scale a prepared bitmap only when its resource changes. Scrolling and
    /// fading update placement without rerasterizing or rescaling the pixels.
    pub fn blend_scaled(
        &self,
        target: &mut frame::Video,
        canvas: (u32, u32),
        cache: &mut SurfaceScaleCache,
    ) -> Result<()> {
        ensure!(
            canvas.0 > 0 && canvas.1 > 0,
            "surface reference canvas must be positive"
        );
        let destination = (target.width(), target.height());
        if canvas == destination {
            self.blend(target);
            return Ok(());
        }
        if cache.geometry != Some((canvas, destination))
            || !cache
                .source
                .as_ref()
                .is_some_and(|source| Arc::ptr_eq(source, &self.surface))
        {
            let width = (u64::from(self.surface.width) * u64::from(destination.0)
                / u64::from(canvas.0))
            .max(2)
                & !1;
            let height = (u64::from(self.surface.height) * u64::from(destination.1)
                / u64::from(canvas.1))
            .max(2)
                & !1;
            ensure!(
                width <= u64::from(cache.max_dimension) && height <= u64::from(cache.max_dimension),
                "scaled surface dimensions are unsupported"
            );
            ensure!(
                width * height * 3 <= cache.budget as u64,
                "scaled surface exceeds cache budget"
            );
            let (width, height) = (width as u32, height as u32);
            let mut scaler = ffmpeg_next::software::scaling::Context::get(
                Pixel::YUVA420P,
                self.surface.width,
                self.surface.height,
                Pixel::YUVA420P,
                width,
                height,
                ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
            )?;
            let mut prepared = frame::Video::empty();
            scaler.run(&self.surface.frame, &mut prepared)?;
            let scaled = Arc::new(OverlayFrame {
                bounds: alpha_bounds(&prepared),
                chroma_alpha: chroma_alpha(&prepared, width, height),
                frame: prepared,
                width,
                height,
                x: 0,
                y: 0,
                opacity: self.surface.opacity,
            });
            let bytes = (0..scaled.frame.planes())
                .map(|plane| scaled.frame.data(plane).len())
                .sum::<usize>()
                + scaled.chroma_alpha.len();
            ensure!(bytes <= cache.budget, "scaled surface exceeds cache budget");
            cache.scaled = Some(scaled);
            cache.source = Some(self.surface.clone());
            cache.geometry = Some((canvas, destination));
        }
        if let Some(surface) = &cache.scaled {
            let snapshot = Self {
                surface: surface.clone(),
                x: even_signed(
                    (i64::from(self.x) * i64::from(destination.0) / i64::from(canvas.0)) as i32,
                ),
                y: even_signed(
                    (i64::from(self.y) * i64::from(destination.1) / i64::from(canvas.1)) as i32,
                ),
                opacity: self.opacity,
            };
            snapshot.blend(target);
        }

        Ok(())
    }
}

fn even_signed(value: i32) -> i32 {
    value & !1
}
