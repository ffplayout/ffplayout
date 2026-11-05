//! Signal-domain RGB/YUV policy for alpha preparation; no implicit gamut or HDR conversion.
use anyhow::{Result, bail, ensure};
use ffmpeg_next::{
    ffi, frame,
    util::color::{Range, Space, TransferCharacteristic},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ColorConversion {
    pub space: Space,
    pub full: bool,
    pub source_full: bool,
    pub coefficients: i32,
    pub kr: f64,
    pub kb: f64,
}

pub(super) fn has_alpha(source: &frame::Video) -> bool {
    has_flag(source, ffi::AV_PIX_FMT_FLAG_ALPHA)
}

fn has_flag(source: &frame::Video, flag: i32) -> bool {
    source.format().descriptor().is_some_and(|descriptor| {
        // SAFETY: FFmpeg owns the static descriptor for this pixel format.
        unsafe { (*descriptor.as_ptr()).flags & flag as u64 != 0 }
    })
}

impl ColorConversion {
    pub fn from_frame(source: &frame::Video) -> Result<Self> {
        ensure!(
            !matches!(
                source.color_transfer_characteristic(),
                TransferCharacteristic::Linear
                    | TransferCharacteristic::SMPTE2084
                    | TransferCharacteristic::ARIB_STD_B67
            ),
            "linear/HDR graphics require an explicit color preparation policy"
        );
        let (space, coefficients, kr, kb) = match source.color_space() {
            Space::BT709 => (Space::BT709, ffi::SWS_CS_ITU709, 0.2126, 0.0722),
            Space::RGB | Space::Unspecified | Space::BT470BG | Space::SMPTE170M => {
                (Space::SMPTE170M, ffi::SWS_CS_ITU601, 0.299, 0.114)
            }
            Space::FCC => (Space::FCC, ffi::SWS_CS_FCC, 0.30, 0.11),
            Space::SMPTE240M => (Space::SMPTE240M, ffi::SWS_CS_SMPTE240M, 0.212, 0.087),
            Space::BT2020NCL => (Space::BT2020NCL, ffi::SWS_CS_BT2020, 0.2627, 0.0593),
            other => bail!("unsupported graphics RGB conversion matrix: {other:?}"),
        };
        let rgb = has_flag(source, ffi::AV_PIX_FMT_FLAG_RGB);

        Ok(Self {
            space,
            coefficients,
            kr,
            kb,
            full: !rgb && source.color_range() == Range::JPEG,
            source_full: rgb || source.color_range() == Range::JPEG,
        })
    }

    pub fn luma(self, rgb: [f64; 3]) -> u8 {
        let luma = self.kr * rgb[0] + (1.0 - self.kr - self.kb) * rgb[1] + self.kb * rgb[2];
        let value = if self.full {
            255.0 * luma
        } else {
            16.0 + 219.0 * luma
        };

        value.round().clamp(0.0, 255.0) as u8
    }

    pub fn chroma(self, rgb: [f64; 3]) -> [u8; 2] {
        let luma = self.kr * rgb[0] + (1.0 - self.kr - self.kb) * rgb[1] + self.kb * rgb[2];
        let cb = (rgb[2] - luma) / (2.0 * (1.0 - self.kb));
        let cr = (rgb[0] - luma) / (2.0 * (1.0 - self.kr));
        let range = if self.full { 255.0 } else { 224.0 };

        [128.0 + range * cb, 128.0 + range * cr].map(|value| value.round().clamp(0.0, 255.0) as u8)
    }
}
