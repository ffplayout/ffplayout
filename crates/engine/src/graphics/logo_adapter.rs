//! Engine adapter: retain media I/O policy while graphics owns logo preparation/layout.
use std::sync::Arc;

use anyhow::Result;
#[cfg(test)]
use anyhow::{Context, ensure};
use ff_graphics::ffmpeg::logo::LogoOverlay as PreparedLogoOverlay;
#[cfg(test)]
use ff_graphics::style::LogoStyle;
use ff_graphics::surface::SurfaceDescriptor;
use ffmpeg_next::frame;

use crate::utils::config::LogoConfig;
#[cfg(test)]
use crate::utils::helper::open_media_input;

#[derive(Clone)]
pub struct LogoOverlay(
    pub(super) Arc<PreparedLogoOverlay>,
    pub(super) Option<Arc<str>>,
);

impl LogoOverlay {
    pub(super) fn descriptor(&self) -> SurfaceDescriptor {
        self.0.descriptor()
    }

    #[cfg(test)]
    pub fn load(config: &LogoConfig, width: u32, height: u32) -> Result<Self> {
        let style = LogoStyle {
            scale: config.scale.clone(),
            opacity: config.opacity,
            position: config.position.clone(),
        };
        ensure!(
            (0.0..=1.0).contains(&config.opacity) && config.opacity.is_finite(),
            "logo opacity must be between 0.0 and 1.0"
        );
        let input = open_media_input(&config.path)
            .with_context(|| format!("failed to open logo {}", config.path))?;
        PreparedLogoOverlay::from_input(&style, input, &config.path, width, height)
            .map(|logo| Self(Arc::new(logo), None))
    }

    pub(super) fn restyled(&self, style: &LogoConfig, width: u32, height: u32) -> Result<Self> {
        let mut logo = self.0.positioned(Some(&style.position), width, height)?;
        logo.opacity = (style.opacity * 255.0).round().clamp(0.0, 255.0) as u8;

        Ok(Self(
            Arc::new(logo),
            Some(Arc::from(style.position.as_str())),
        ))
    }

    pub fn width(&self) -> u32 {
        self.0.width
    }

    pub fn height(&self) -> u32 {
        self.0.height
    }

    #[cfg(test)]
    pub(crate) fn position(&self) -> (u32, u32) {
        (self.0.x, self.0.y)
    }

    pub fn opacity(&self) -> u8 {
        self.0.opacity
    }

    pub(crate) fn positioned(
        &self,
        position: Option<&str>,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        self.0
            .positioned(self.1.as_deref().or(position), width, height)
            .map(|logo| Self(Arc::new(logo), self.1.clone()))
    }
}

pub fn blend_logo(target: &mut frame::Video, logo: &LogoOverlay, opacity: f64) {
    ff_graphics::ffmpeg::logo::blend_logo(target, &logo.0, opacity);
}
