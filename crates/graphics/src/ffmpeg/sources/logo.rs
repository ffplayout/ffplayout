use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{frame, util::format::pixel::Pixel};

use crate::{
    even,
    ffmpeg::{
        compositor::metadata::describe_yuva,
        compositor::overlay::{
            OverlayBounds, OverlayRef, alpha_bounds, blend_overlay, chroma_alpha, prepare_execution,
        },
        reference_video_frame,
    },
    layout::logo_position,
    preparation::{ResourceBudget, ResourceLease},
    style::LogoStyle,
    surface::{ColorEncoding, SurfaceDescriptor},
};

use super::decoder::LogoDecoder;

#[derive(Clone)]
pub struct LogoOverlay {
    lease: Option<ResourceLease>,
    pub frame: frame::Video, // YUVA420P
    chroma_alpha: Arc<[u8]>,
    bounds: OverlayBounds,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub opacity: u8, // 0..255
}

impl LogoOverlay {
    pub fn descriptor(&self) -> SurfaceDescriptor {
        describe_yuva(
            &self.frame,
            self.bounds,
            self.chroma_alpha.len(),
            ColorEncoding::Unspecified,
        )
    }

    /// Reuse the decoded logo pixels; only placement depends on the destination.
    pub fn positioned(&self, position: Option<&str>, width: u32, height: u32) -> Result<Self> {
        let (x, y) = if let Some(position) = position {
            logo_position(position, width, height, self.width, self.height)?
        } else {
            (self.x, self.y)
        };

        Ok(Self {
            lease: self.lease.clone(),
            frame: reference_video_frame(&self.frame)?,
            chroma_alpha: self.chroma_alpha.clone(),
            bounds: self.bounds,
            x: even(x),
            y: even(y),
            width: self.width,
            height: self.height,
            opacity: self.opacity,
        })
    }

    pub fn from_input(
        config: &LogoStyle,
        ictx: ffmpeg_next::format::context::Input,
        source_label: &str,
        output_width: u32,
        output_height: u32,
    ) -> Result<Self> {
        if !(0.0..=1.0).contains(&config.opacity) || !config.opacity.is_finite() {
            return Err(anyhow!("logo opacity must be between 0.0 and 1.0"));
        }

        let mut decoder = LogoDecoder::new(ictx, config.clone(), output_width, output_height)
            .with_context(|| format!("opening logo {source_label}"))?;
        let first = decoder
            .next_frame()
            .with_context(|| format!("decoding logo {source_label}"))?
            .with_context(|| format!("logo {source_label} produced no frame"))?;

        Ok(Arc::unwrap_or_clone(first.surface))
    }

    /// Prepare an already scaled YUVA surface outside the compositor hot path.
    pub fn from_yuva(
        config: &LogoStyle,
        frame: frame::Video,
        output_width: u32,
        output_height: u32,
    ) -> Result<Self> {
        anyhow::ensure!(
            frame.format() == Pixel::YUVA420P,
            "logo surface must be YUVA420P"
        );
        anyhow::ensure!(
            (0.0..=1.0).contains(&config.opacity) && config.opacity.is_finite(),
            "invalid logo opacity"
        );
        let width = frame.width();
        let height = frame.height();
        let (x, y) = logo_position(&config.position, output_width, output_height, width, height)?;
        let bounds = alpha_bounds(&frame);
        if config.opacity > 0.0 {
            prepare_execution(bounds);
        }

        Ok(Self {
            lease: None,
            chroma_alpha: chroma_alpha(&frame, width, height).into(),
            bounds,
            frame,
            x: even(x),
            y: even(y),
            width,
            height,
            opacity: (config.opacity * 255.0).round() as u8,
        })
    }

    pub fn with_budget(mut self, budget: &ResourceBudget) -> Result<Self> {
        self.lease = Some(budget.reserve(self.storage_bytes())?);

        Ok(self)
    }

    pub fn storage_bytes(&self) -> usize {
        (0..self.frame.planes())
            .map(|plane| self.frame.data(plane).len())
            .sum::<usize>()
            .saturating_add(self.chroma_alpha.len())
    }

    pub fn as_overlay(&self) -> OverlayRef<'_> {
        OverlayRef {
            frame: &self.frame,
            bounds: self.bounds,
            chroma_alpha: &self.chroma_alpha,
            chroma_alpha_stride: self.width as usize / 2,
            x: self.x as i32,
            y: self.y as i32,
            width: self.width,
            height: self.height,
            opacity: self.opacity,
        }
    }
}

pub fn blend_logo(target: &mut frame::Video, logo: &LogoOverlay, opacity_factor: f64) {
    blend_overlay(target, logo.as_overlay(), opacity_factor);
}
