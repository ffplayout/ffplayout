//! SIMD RGB16 filtering over borrowed FFmpeg rows, without packing/copying the frame.
use std::num::NonZeroU32;

use anyhow::{Context, Result};
use fast_image_resize::{
    FilterType, ImageView, ImageViewMut, MulDiv, ResizeAlg, ResizeOptions, Resizer,
    images::{TypedCroppedImage, TypedCroppedImageMut, TypedImage, TypedImageRef},
    pixels::U16x4,
};
use ffmpeg_next::{frame, util::format::pixel::Pixel};
use rayon::{ThreadPool, prelude::*};

#[derive(Default)]
pub(super) struct RgbaResizer {
    resizer: Resizer,
    multiply: MulDiv,
    bands: Vec<Resizer>,
    geometry: Option<((u32, u32), (u32, u32))>,
}

impl RgbaResizer {
    pub fn reset_geometry(&mut self, source: (u32, u32), destination: (u32, u32)) {
        if self.geometry != Some((source, destination)) {
            self.resizer.reset_internal_buffers();
            self.bands.clear();
            self.geometry = Some((source, destination));
        }
    }

    pub fn associate(&self, frame: &mut frame::Video, pool: Option<&ThreadPool>) -> Result<()> {
        let (width, height) = (frame.width(), frame.height());
        let stride = frame.stride(0) as u32 / 8;
        let mut image = TypedImage::<U16x4>::from_buffer(stride, height, frame.data_mut(0))?;
        let mut visible = TypedCroppedImageMut::from_ref(&mut image, 0, 0, width, height)?;

        if let Some(pool) = pool {
            let count = pool.current_num_threads().min(4).min(height as usize) as u32;
            let mut bands = visible
                .split_by_height_mut(
                    0,
                    NonZeroU32::new(height).context("empty graphics image")?,
                    NonZeroU32::new(count).context("empty graphics executor")?,
                )
                .context("splitting graphics alpha rows")?;
            pool.install(|| {
                bands
                    .par_iter_mut()
                    .try_for_each(|band| self.multiply.multiply_alpha_inplace_typed(band))
            })?;
        } else {
            self.multiply.multiply_alpha_inplace_typed(&mut visible)?;
        }

        Ok(())
    }

    pub fn run(
        &mut self,
        source: &frame::Video,
        size: (u32, u32),
        pool: Option<&ThreadPool>,
    ) -> Result<frame::Video> {
        let image = TypedImageRef::<U16x4>::from_buffer(
            source.stride(0) as u32 / 8,
            source.height(),
            source.data(0),
        )?;
        let visible = TypedCroppedImage::from_ref(&image, 0, 0, source.width(), source.height())?;
        let mut output = frame::Video::new(Pixel::RGBA64LE, size.0, size.1);
        let mut image = TypedImage::<U16x4>::from_buffer(
            output.stride(0) as u32 / 8,
            size.1,
            output.data_mut(0),
        )?;
        let mut destination = TypedCroppedImageMut::from_ref(&mut image, 0, 0, size.0, size.1)?;
        // Input is already associated. A second alpha multiply would darken edges.
        // Convolution expands bilinear support when downscaling (antialiasing).
        let options = ResizeOptions::new()
            .resize_alg(ResizeAlg::Convolution(FilterType::Bilinear))
            .use_alpha(false);

        if let Some(pool) = pool {
            let count = pool.current_num_threads().min(4).min(size.1 as usize);
            self.bands.resize_with(count, Resizer::new);
            let mut destinations = destination
                .split_by_height_mut(
                    0,
                    NonZeroU32::new(size.1).context("empty graphics image")?,
                    NonZeroU32::new(count as u32).context("empty graphics executor")?,
                )
                .context("splitting graphics resize rows")?;
            let mut top = 0;
            let options: Vec<_> = destinations
                .iter()
                .map(|band| {
                    let start = f64::from(top) * f64::from(source.height()) / f64::from(size.1);
                    top += band.height();
                    let end = f64::from(top) * f64::from(source.height()) / f64::from(size.1);

                    // Keep the full source view: filters may read the halo beyond a
                    // band's crop, preserving coverage and phase across boundaries.
                    options.crop(0.0, start, f64::from(source.width()), end - start)
                })
                .collect();
            pool.install(|| {
                self.bands
                    .par_iter_mut()
                    .zip(destinations.par_iter_mut())
                    .zip(options.par_iter())
                    .try_for_each(|((resizer, band), options)| {
                        resizer.resize_typed(&visible, band, options)
                    })
            })?;
        } else {
            self.resizer
                .resize_typed(&visible, &mut destination, &options)?;
        }

        Ok(output)
    }

    #[cfg(test)]
    pub fn storage_bytes(&self) -> usize {
        self.resizer.size_of_internal_buffers()
            + self
                .bands
                .iter()
                .map(Resizer::size_of_internal_buffers)
                .sum::<usize>()
    }
}
