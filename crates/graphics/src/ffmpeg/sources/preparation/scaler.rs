use anyhow::{Context, Result, ensure};
use ffmpeg_next::{ffi, frame, software::scaling, util::format::pixel::Pixel};

use super::color::ColorConversion;

#[derive(Default)]
pub(super) struct Scaler {
    context: Option<scaling::Context>,
    conversion: Option<ColorConversion>,
}

impl Scaler {
    pub fn run(
        &mut self,
        source: &frame::Video,
        format: Pixel,
        size: (u32, u32),
        conversion: Option<ColorConversion>,
    ) -> Result<frame::Video> {
        let changed = self.conversion != conversion
            || self.context.as_ref().is_none_or(|context| {
                let input = context.input();
                let output = context.output();
                input.format != source.format()
                    || input.width != source.width()
                    || input.height != source.height()
                    || output.format != format
                    || output.width != size.0
                    || output.height != size.1
            });

        if changed {
            // Keep the established no-normalization path byte compatible.
            let flags = if format == Pixel::YUVA420P {
                scaling::flag::Flags::BILINEAR
            } else {
                scaling::flag::Flags::BILINEAR | scaling::flag::Flags::ACCURATE_RND
            };
            let mut context = scaling::Context::get(
                source.format(),
                source.width(),
                source.height(),
                format,
                size.0,
                size.1,
                flags,
            )?;

            if let Some(policy) = conversion {
                // SAFETY: context is exclusively owned; FFmpeg returns a static coefficient table.
                let result = unsafe {
                    let coefficients = ffi::sws_getCoefficients(policy.coefficients);
                    ffi::sws_setColorspaceDetails(
                        context.as_mut_ptr(),
                        coefficients,
                        i32::from(policy.source_full),
                        coefficients,
                        1,
                        0,
                        1 << 16,
                        1 << 16,
                    )
                };
                ensure!(
                    result >= 0,
                    "setting graphics RGB conversion matrix/range failed"
                );
            }

            self.context = Some(context);
            self.conversion = conversion;
        }

        let mut output = frame::Video::empty();
        self.context
            .as_mut()
            .context("graphics scaler unavailable")?
            .run(source, &mut output)?;

        Ok(output)
    }
}
