//! Shared immutable CPU surfaces and per-resolution scaling caches.
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use ffmpeg_next::{format::Pixel, frame};

use crate::{
    layout::{LayoutContext, PixelSizing},
    surface::{ColorEncoding, SurfaceDescriptor},
};

use super::metadata::{copy_color_properties, describe_yuva};
use super::overlay::{OverlayFrame, alpha_bounds, blend_overlay, chroma_alpha};

/// Immutable pixels and per-program-frame placement; safe to retain in bounded queues.
#[derive(Clone)]
pub struct SurfaceFrame {
    pub(in crate::ffmpeg) surface: Arc<OverlayFrame>,
    pub(in crate::ffmpeg) x: i32,
    pub(in crate::ffmpeg) y: i32,
    pub(in crate::ffmpeg) opacity: f64,
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
    pub(in crate::ffmpeg) fn storage_bytes(&self) -> usize {
        self.scaled.as_ref().map_or(0, |surface| {
            (0..surface.frame.planes())
                .map(|plane| surface.frame.data(plane).len())
                .sum::<usize>()
                + surface.chroma_alpha.len()
        })
    }

    pub(in crate::ffmpeg) fn set_limits(&mut self, bytes: usize, max_dimension: u32) {
        self.budget = bytes;
        self.max_dimension = max_dimension;
    }
}

impl SurfaceFrame {
    pub fn same_surface(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.surface, &other.surface)
    }

    /// Refresh only evaluated placement/fade, retaining the prepared pixel lease.
    /// A different bitmap must go through resource preparation/activation first.
    pub fn update_presentation(&mut self, other: &Self) -> bool {
        if !self.same_surface(other) {
            return false;
        }
        self.x = other.x;
        self.y = other.y;
        self.opacity = other.opacity;

        true
    }

    /// Ready metadata only: no pixel scan, conversion or rasterization.
    pub fn descriptor(&self, color: ColorEncoding) -> SurfaceDescriptor {
        describe_yuva(
            &self.surface.frame,
            self.surface.bounds,
            self.surface.chroma_alpha.len(),
            color,
        )
    }

    pub fn with_opacity_factor(mut self, factor: f64) -> Self {
        self.opacity *= factor.clamp(0.0, 1.0);

        self
    }

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
            let ((width, height), _) = LayoutContext::new(canvas, destination)?.yuva_geometry(
                self.dimensions(),
                (self.x, self.y),
                PixelSizing::ReferenceCanvas,
            )?;
            ensure!(
                width <= cache.max_dimension && height <= cache.max_dimension,
                "scaled surface dimensions are unsupported"
            );
            let (estimated_bytes, workspace_bytes) = scaling_bytes(width, height)?;
            ensure!(
                estimated_bytes <= cache.budget,
                "scaled surface exceeds cache budget"
            );
            let _workspace = self
                .surface
                .lease
                .as_ref()
                .map(|lease| lease.reserve_related(workspace_bytes))
                .transpose()?;
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
            copy_color_properties(&self.surface.frame, &mut prepared);
            let coverage_bytes = usize::try_from(width / 2)?
                .checked_mul(usize::try_from(height / 2)?)
                .context("scaled chroma coverage byte count overflow")?;
            let bytes = (0..prepared.planes())
                .try_fold(coverage_bytes, |bytes, plane| {
                    bytes.checked_add(prepared.data(plane).len())
                })
                .context("scaled surface byte count overflow")?;
            let lease = self
                .surface
                .lease
                .as_ref()
                .map(|lease| lease.reserve_related(bytes))
                .transpose()?;
            let scaled = Arc::new(OverlayFrame {
                lease,
                bounds: alpha_bounds(&prepared),
                chroma_alpha: chroma_alpha(&prepared, width, height),
                frame: prepared,
                width,
                height,
                x: 0,
                y: 0,
                opacity: self.surface.opacity,
            });
            ensure!(bytes <= cache.budget, "scaled surface exceeds cache budget");
            cache.scaled = Some(scaled);
            cache.source = Some(self.surface.clone());
            cache.geometry = Some((canvas, destination));
        }
        if let Some(surface) = &cache.scaled {
            let (_, (x, y)) = LayoutContext::new(canvas, destination)?.yuva_geometry(
                self.dimensions(),
                (self.x, self.y),
                PixelSizing::ReferenceCanvas,
            )?;
            let snapshot = Self {
                surface: surface.clone(),
                x,
                y,
                opacity: self.opacity,
            };
            snapshot.blend(target);
        }

        Ok(())
    }
}

/// Conservative YUVA/coverage storage plus row-alignment workspace. Compute in
/// checked allocation-sized units before reserving memory or invoking FFmpeg.
fn scaling_bytes(width: u32, height: u32) -> Result<(usize, usize)> {
    let width = usize::try_from(width)?;
    let height = usize::try_from(height)?;
    let image = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
        .context("scaled surface byte estimate overflow")?;
    let workspace = height
        .checked_mul(256)
        .and_then(|padding| image.checked_add(padding))
        .context("scaled surface workspace estimate overflow")?;

    Ok((image, workspace))
}

#[cfg(test)]
mod tests {
    use crate::{ffmpeg::overlay::OverlayBounds, preparation::ResourceBudget};

    use super::*;

    #[test]
    fn extreme_reference_scaling_is_rejected_before_allocating_or_changing_video() {
        let budget = ResourceBudget::new(256 * 1024 * 1024, 16, 1);
        let source = SurfaceFrame {
            surface: Arc::new(OverlayFrame {
                lease: Some(budget.reserve(100_000).unwrap()),
                frame: frame::Video::new(Pixel::YUVA420P, 1024, 16),
                chroma_alpha: vec![255; 1024 * 16 / 4],
                bounds: OverlayBounds::full(1024, 16),
                width: 1024,
                height: 16,
                x: 0,
                y: 0,
                opacity: 255,
            }),
            x: 0,
            y: 0,
            opacity: 1.0,
        };
        // Scale by 512: the prepared image would be 524288 x 8192, exceeding
        // u32 arithmetic and the channel budget while the target stays small.
        let mut target = frame::Video::new(Pixel::YUV420P, 1024, 1024);
        target.data_mut(0).fill(70);
        let before = budget.usage();
        let mut cache = SurfaceScaleCache::default();
        assert!(
            source
                .blend_scaled(&mut target, (2, 2), &mut cache)
                .is_err()
        );
        assert_eq!(budget.usage(), before);
        assert!(cache.scaled.is_none());
        assert!(target.data(0).iter().all(|&value| value == 70));
    }

    #[test]
    fn scaling_estimates_use_checked_allocation_sized_arithmetic() {
        assert_eq!(scaling_bytes(1280, 720).unwrap(), (2_764_800, 2_949_120));
        assert!(scaling_bytes(u32::MAX, u32::MAX).is_err());
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            scaling_bytes(524_288, 8192).unwrap(),
            (12_884_901_888, 12_886_999_040)
        );
        #[cfg(target_pointer_width = "32")]
        assert!(scaling_bytes(524_288, 8192).is_err());
    }
}
