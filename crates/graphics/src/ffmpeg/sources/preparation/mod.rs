//! Source-side alpha normalization. Runs on preparation workers, not the compositor.
use anyhow::{Result, ensure};
use ffmpeg_next::{frame, util::format::pixel::Pixel};
use rayon::ThreadPool;

#[cfg(ff_graphics_av_alpha_mode)]
use ffmpeg_next::{ffi, ffi::AVAlphaMode, util::color::TransferCharacteristic};

mod color;
mod kernels;
mod resize;
mod rgb;
mod rgba;
mod scaler;

use crate::{ffmpeg::execution, surface::Alpha};

use self::{
    color::ColorConversion,
    resize::RgbaResizer,
    rgba::{associate, attach_alpha, to_yuva},
    scaler::Scaler,
};

/// The cached CPU adapter accepts encoded premultiplication. The scene-level
/// alpha enum remains independent of this backend and its SDR conversion policy.
#[derive(Default)]
pub struct SourcePreparation {
    direct: Scaler,
    rgb: Scaler,
    mask: Scaler,
    resize: Scaler,
    optimized: RgbaResizer,
    rgb_buffer: Option<frame::Video>,
    mask_buffer: Option<frame::Video>,
    resize_buffer: Option<frame::Video>,
}

/// Explicit reference mode is for regression tests and performance comparisons.
#[derive(Clone, Copy, Default)]
pub enum PreparationExecution<'a> {
    Reference,
    /// Fast straight-alpha scaling. Premultiplied normalization is unaffected.
    Standard,
    Serial,
    Parallel(&'a ThreadPool),
    #[default]
    Adaptive,
}

impl SourcePreparation {
    pub(super) fn release_workspace(&mut self) {
        self.rgb_buffer = None;
        self.mask_buffer = None;
        self.resize_buffer = None;
        self.optimized = RgbaResizer::default();
    }

    pub fn prepare(
        &mut self,
        source: &frame::Video,
        auxiliary: Option<&frame::Video>,
        alpha: Alpha,
        size: (u32, u32),
    ) -> Result<frame::Video> {
        self.prepare_with_execution(
            source,
            auxiliary,
            alpha,
            size,
            PreparationExecution::Adaptive,
        )
    }

    /// Standard scaling may filter straight color and alpha independently, but
    /// encoded-premultiplied input is always normalized correctly.
    pub fn prepare_with_quality(
        &mut self,
        source: &frame::Video,
        auxiliary: Option<&frame::Video>,
        alpha: Alpha,
        size: (u32, u32),
        high_quality: bool,
    ) -> Result<frame::Video> {
        if !high_quality && alpha == Alpha::Straight {
            // Reuse the validated direct path, including separate alpha planes.
            self.prepare_with_execution(
                source,
                auxiliary,
                alpha,
                size,
                PreparationExecution::Standard,
            )
        } else {
            self.prepare(source, auxiliary, alpha, size)
        }
    }

    pub fn prepare_with_execution(
        &mut self,
        source: &frame::Video,
        auxiliary: Option<&frame::Video>,
        alpha: Alpha,
        size: (u32, u32),
        execution: PreparationExecution<'_>,
    ) -> Result<frame::Video> {
        let reference =
            matches!(execution, PreparationExecution::Reference) || cfg!(target_endian = "big");
        ensure!(
            size.0 > 0
                && size.1 > 0
                && size.0 <= 8192
                && size.1 <= 8192
                && size.0.is_multiple_of(2)
                && size.1.is_multiple_of(2),
            "invalid graphics preparation size"
        );
        ensure!(
            source.width() > 0
                && source.height() > 0
                && source.width() <= 8192
                && source.height() <= 8192,
            "invalid graphics source dimensions"
        );
        if let Some(mask) = auxiliary {
            ensure!(
                mask.width() == source.width() && mask.height() == source.height(),
                "graphics alpha dimensions do not match color"
            );
            ensure!(
                mask.format()
                    .descriptor()
                    .is_some_and(|format| format.nb_components() == 1),
                "graphics alpha image must be monochrome"
            );
        }

        self.optimized
            .reset_geometry((source.width(), source.height()), size);
        let resize = size != (source.width(), source.height());
        let has_alpha = auxiliary.is_some() || color::has_alpha(source);

        if alpha == Alpha::Straight
            && (!resize || !has_alpha || matches!(execution, PreparationExecution::Standard))
        {
            self.rgb_buffer = None;
            self.resize_buffer = None;
            let mut output = self.direct.run(source, Pixel::YUVA420P, size, None)?;

            if let Some(mask) = auxiliary {
                let buffer = self.mask_buffer.get_or_insert_with(frame::Video::empty);

                self.mask.run_into(mask, Pixel::GRAY8, size, None, buffer)?;
                rgba::attach_alpha8(buffer, &mut output);
            } else {
                self.mask_buffer = None;
            }

            mark_straight(&mut output);

            return Ok(output);
        }

        ensure!(
            alpha != Alpha::PremultipliedLinear,
            "linear-premultiplied graphics require a linear color preparation policy"
        );
        let source_pixels = u64::from(source.width()) * u64::from(source.height());
        let target_pixels = u64::from(size.0) * u64::from(size.1);
        ensure!(
            source_pixels * 10
                + target_pixels * 12
                + if resize && !reference {
                    // Serial and four band workspaces may both be cached.
                    u64::from(size.0) * (u64::from(source.height()) + 8) * 16 + 1024 * 1024
                } else {
                    0
                }
                <= 256 * 1024 * 1024,
            "alpha preparation workspace exceeds 256 MiB; reduce source size"
        );
        let conversion = ColorConversion::from_frame(source)?;

        if auxiliary.is_none() {
            self.mask_buffer = None;
        }

        // Already-associated RGB16 only needs a read-only view of decoder pixels.
        if !reference
            && source.format() == Pixel::RGBA64LE
            && alpha == Alpha::PremultipliedEncoded
            && auxiliary.is_none()
        {
            self.rgb_buffer = None;
        } else {
            let rgb = self.rgb_buffer.get_or_insert_with(frame::Video::empty);
            let source_size = (source.width(), source.height());

            if reference || !rgb::convert(source, rgb) {
                self.rgb
                    .run_into(source, Pixel::RGBA64LE, source_size, Some(conversion), rgb)?;
            }

            if let Some(mask) = auxiliary {
                let buffer = self.mask_buffer.get_or_insert_with(frame::Video::empty);

                self.mask
                    .run_into(mask, Pixel::GRAY16LE, source_size, None, buffer)?;
                attach_alpha(buffer, rgb);
            }
        }

        let permit = if matches!(execution, PreparationExecution::Adaptive)
            && source_pixels.max(target_pixels) >= 3_500_000
        {
            execution::try_shared()
        } else {
            None
        };
        let pool = match execution {
            PreparationExecution::Parallel(pool) => Some(pool),
            PreparationExecution::Adaptive => permit.as_ref().map(|permit| permit.pool),
            _ => None,
        };

        let rgb = if let Some(rgb) = self.rgb_buffer.as_mut() {
            if resize && alpha == Alpha::Straight {
                if reference {
                    associate(rgb);
                } else {
                    self.optimized.associate(rgb, pool)?;
                }
            }

            &*rgb
        } else {
            source
        };
        let rgb = if resize {
            let scaled = self.resize_buffer.get_or_insert_with(frame::Video::empty);

            if reference {
                self.resize
                    .run_into(rgb, Pixel::RGBA64LE, size, None, scaled)?;
            } else {
                self.optimized.run(rgb, size, pool, scaled)?;
            }

            scaled
        } else {
            self.resize_buffer = None;
            rgb
        };

        let mut output = if reference {
            rgba::to_yuva_reference(rgb, source, conversion)?
        } else {
            to_yuva(rgb, source, conversion, pool)?
        };
        mark_straight(&mut output);

        Ok(output)
    }
}

fn ensure_rgba_storage(frame: &mut frame::Video, size: (u32, u32)) {
    if (frame.width(), frame.height()) != size || frame.format() != Pixel::RGBA64LE {
        *frame = frame::Video::new(Pixel::RGBA64LE, size.0, size.1);
    }
}

fn mark_straight(frame: &mut frame::Video) {
    #[cfg(ff_graphics_av_alpha_mode)]
    // SAFETY: frame is exclusively borrowed and the build checked this field.
    unsafe {
        (*frame.as_mut_ptr()).alpha_mode = ffi::AVAlphaMode::AVALPHA_MODE_STRAIGHT;
    }

    #[cfg(not(ff_graphics_av_alpha_mode))]
    let _ = frame;
}

/// Decoder output is authoritative: a decoder may already have unassociated
/// a premultiplied source. Stream metadata is only a fallback for unset frames.
pub(super) fn frame_alpha(frame: &frame::Video) -> Option<Alpha> {
    #[cfg(ff_graphics_av_alpha_mode)]
    {
        // SAFETY: the frame owns this AVFrame and the build checked the header.
        let mode = unsafe { (*frame.as_ptr()).alpha_mode };

        alpha_mode(mode, frame.color_transfer_characteristic())
    }

    #[cfg(not(ff_graphics_av_alpha_mode))]
    {
        let _ = frame;

        None
    }
}

pub(super) fn stream_alpha(stream: &ffmpeg_next::format::stream::Stream<'_>) -> Option<Alpha> {
    #[cfg(ff_graphics_av_alpha_mode)]
    {
        let parameters = stream.parameters();
        // SAFETY: parameters keeps the referenced AVCodecParameters alive.
        let (mode, transfer) = unsafe {
            (
                (*parameters.as_ptr()).alpha_mode,
                (*parameters.as_ptr()).color_trc,
            )
        };

        alpha_mode(mode, transfer.into())
    }

    #[cfg(not(ff_graphics_av_alpha_mode))]
    {
        let _ = stream;

        None
    }
}

#[cfg(ff_graphics_av_alpha_mode)]
fn alpha_mode(mode: AVAlphaMode, transfer: TransferCharacteristic) -> Option<Alpha> {
    match mode {
        AVAlphaMode::AVALPHA_MODE_PREMULTIPLIED if transfer == TransferCharacteristic::Linear => {
            Some(Alpha::PremultipliedLinear)
        }
        AVAlphaMode::AVALPHA_MODE_PREMULTIPLIED => Some(Alpha::PremultipliedEncoded),
        AVAlphaMode::AVALPHA_MODE_STRAIGHT => Some(Alpha::Straight),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
