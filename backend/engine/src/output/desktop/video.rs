use std::sync::Arc;

use anyhow::{Result, anyhow};
#[cfg(feature = "desktop-gpu")]
use ffmpeg_next::util::color;
use ffmpeg_next::{frame, software::scaling, util::format::pixel::Pixel};

#[derive(Clone)]
pub(super) struct VideoSurface {
    pub(super) width: u32,
    pub(super) height: u32,
    #[cfg(feature = "desktop-gpu")]
    pub(super) y: Arc<[u8]>,
    #[cfg(feature = "desktop-gpu")]
    pub(super) u: Arc<[u8]>,
    #[cfg(feature = "desktop-gpu")]
    pub(super) v: Arc<[u8]>,
    #[cfg(feature = "desktop-gpu")]
    pub(super) color_space: color::Space,
    #[cfg(feature = "desktop-gpu")]
    pub(super) color_range: color::Range,
    #[cfg(feature = "desktop-cpu")]
    pub(super) pixels: Arc<[u32]>,
    pub(super) pts: i64,
}

pub(super) struct DesktopFrameConverter {
    scaler: Option<scaling::Context>,
    converted: frame::Video,
}

impl Default for DesktopFrameConverter {
    fn default() -> Self {
        Self {
            scaler: None,
            converted: frame::Video::empty(),
        }
    }
}

impl DesktopFrameConverter {
    pub(super) fn convert(&mut self, frame: &frame::Video) -> Result<VideoSurface> {
        let width = frame.width();
        let height = frame.height();
        if width == 0 || height == 0 {
            return Err(anyhow!("desktop video frame has zero dimensions"));
        }

        let reconfigure = self.scaler.as_ref().is_none_or(|scaler| {
            let input = scaler.input();
            input.format != frame.format() || input.width != width || input.height != height
        });
        if reconfigure {
            if let Some(scaler) = &mut self.scaler {
                scaler.cached(
                    frame.format(),
                    width,
                    height,
                    output_pixel_format(),
                    width,
                    height,
                    scaling::flag::Flags::FAST_BILINEAR,
                );
            } else {
                self.scaler = Some(scaling::Context::get(
                    frame.format(),
                    width,
                    height,
                    output_pixel_format(),
                    width,
                    height,
                    scaling::flag::Flags::FAST_BILINEAR,
                )?);
            }
            self.converted = frame::Video::empty();
        }
        self.scaler
            .as_mut()
            .expect("desktop scaler must be initialized")
            .run(frame, &mut self.converted)?;

        #[cfg(feature = "desktop-cpu")]
        {
            let mut pixels = vec![0_u32; width as usize * height as usize];
            let stride = self.converted.stride(0) / 4;
            for (target_row, source_row) in pixels
                .chunks_exact_mut(width as usize)
                .zip(self.converted.plane::<[u8; 4]>(0).chunks_exact(stride))
            {
                for (target, source) in target_row.iter_mut().zip(source_row) {
                    *target = bgrz_to_rgb_pixel(*source);
                }
            }
            Ok(VideoSurface {
                width,
                height,
                pixels: pixels.into(),
                pts: frame.pts().unwrap_or_default(),
            })
        }

        #[cfg(feature = "desktop-gpu")]
        {
            let chroma_width = width.div_ceil(2) as usize;
            let chroma_height = height.div_ceil(2) as usize;
            Ok(VideoSurface {
                width,
                height,
                y: copy_frame_plane(&self.converted, 0, width as usize, height as usize).into(),
                u: copy_frame_plane(&self.converted, 1, chroma_width, chroma_height).into(),
                v: copy_frame_plane(&self.converted, 2, chroma_width, chroma_height).into(),
                color_space: frame.color_space(),
                color_range: frame.color_range(),
                pts: frame.pts().unwrap_or_default(),
            })
        }
    }
}

#[cfg(feature = "desktop-cpu")]
fn output_pixel_format() -> Pixel {
    Pixel::BGRZ
}

#[cfg(feature = "desktop-gpu")]
fn output_pixel_format() -> Pixel {
    Pixel::YUV420P
}

#[cfg(feature = "desktop-gpu")]
fn copy_frame_plane(frame: &frame::Video, plane: usize, width: usize, height: usize) -> Vec<u8> {
    let stride = frame.stride(plane);
    let source = frame.data(plane);
    let mut pixels = vec![0; width * height];
    for (target, source) in pixels
        .chunks_exact_mut(width)
        .zip(source.chunks_exact(stride))
    {
        target.copy_from_slice(&source[..width]);
    }
    pixels
}

#[cfg(feature = "desktop-cpu")]
pub(super) fn bgrz_to_rgb_pixel([blue, green, red, _]: [u8; 4]) -> u32 {
    (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue)
}
