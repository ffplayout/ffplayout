//! Read prepared CPU frame metadata once, without scanning/copying pixels.
use anyhow::{Result, ensure};
use ffmpeg_next::{
    ffi,
    format::Pixel,
    frame,
    util::{chroma::Location, color::Range},
};

use crate::surface::{
    Alpha, ChromaSiting, ColorEncoding, ColorProperties, PixelLayout, PlaneDescriptor, SignalRange,
    SurfaceDescriptor, SurfaceMetadata, VisibleRect,
};

use super::overlay::OverlayBounds;

pub fn color_properties(frame: &frame::Video) -> ColorProperties {
    let matrix: ffi::AVColorSpace = frame.color_space().into();
    let primaries: ffi::AVColorPrimaries = frame.color_primaries().into();
    let transfer: ffi::AVColorTransferCharacteristic = frame.color_transfer_characteristic().into();

    ColorProperties {
        matrix: matrix as u32,
        primaries: primaries as u32,
        transfer: transfer as u32,
        range: match frame.color_range() {
            Range::Unspecified => SignalRange::Unspecified,
            Range::MPEG => SignalRange::Limited,
            Range::JPEG => SignalRange::Full,
        },
        chroma: match frame.chroma_location() {
            Location::Unspecified => ChromaSiting::Unspecified,
            Location::Left => ChromaSiting::Left,
            Location::Center => ChromaSiting::Center,
            Location::TopLeft => ChromaSiting::TopLeft,
            Location::Top => ChromaSiting::Top,
            Location::BottomLeft => ChromaSiting::BottomLeft,
            Location::Bottom => ChromaSiting::Bottom,
        },
    }
}

/// The compatibility label is a preparation policy, not a substitute for the
/// actual frame tags. Explicit contradictory matrix/range labels are rejected.
pub fn yuva_descriptor(
    frame: &frame::Video,
    bounds: OverlayBounds,
    coverage_bytes: usize,
    color: ColorEncoding,
) -> Result<SurfaceDescriptor> {
    ensure!(
        frame.format() == Pixel::YUVA420P,
        "CPU graphics require YUVA420P"
    );
    let properties = color_properties(frame);
    let expected_matrix = match color {
        ColorEncoding::Bt601Limited => Some([5, 6]),
        ColorEncoding::Bt709Limited => Some([1, 1]),
        ColorEncoding::Unspecified => None,
        ColorEncoding::Srgb => anyhow::bail!("sRGB is not a YUVA signal encoding"),
    };
    if let Some(matrices) = expected_matrix {
        ensure!(
            properties.matrix == 2 || matrices.contains(&properties.matrix),
            "prepared matrix contradicts its signal encoding"
        );
        ensure!(
            properties.range != SignalRange::Full,
            "prepared range contradicts its signal encoding"
        );
    }
    let descriptor = describe_yuva(frame, bounds, coverage_bytes, color);
    descriptor.validate()?;

    Ok(descriptor)
}

pub(in crate::ffmpeg) fn describe_yuva(
    frame: &frame::Video,
    bounds: OverlayBounds,
    coverage_bytes: usize,
    color: ColorEncoding,
) -> SurfaceDescriptor {
    let properties = color_properties(frame);
    let planes = std::array::from_fn(|index| {
        Some(PlaneDescriptor {
            width: frame.plane_width(index),
            height: frame.plane_height(index),
            stride_bytes: frame.stride(index),
            storage_bytes: frame.data(index).len(),
        })
    });
    SurfaceDescriptor {
        width: frame.width(),
        height: frame.height(),
        layout: PixelLayout::Yuva420p8,
        alpha: Alpha::Straight,
        color,
        storage_bytes: planes
            .iter()
            .flatten()
            .map(|plane| plane.storage_bytes)
            .sum::<usize>()
            + coverage_bytes,
        metadata: Some(SurfaceMetadata {
            planes,
            visible: VisibleRect {
                x: bounds.x as u32,
                y: bounds.y as u32,
                width: bounds.width as u32,
                height: bounds.height as u32,
            },
            color: properties,
        }),
    }
}

/// Existing kernels blend encoded signal values. This policy deliberately
/// preserves legacy output; it performs neither color conversion nor inference.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorPolicy {
    LegacySignal,
    /// Strict admission requires matching SDR tags and centered chroma.
    /// Unsupported or unknown tags require preparation before take.
    MatchingSignal,
}

impl ColorPolicy {
    pub fn supports(self, source: ColorProperties, target: ColorProperties) -> bool {
        self == Self::LegacySignal
            || (source == target
                && [1, 5, 6].contains(&source.matrix)
                && [1, 5, 6].contains(&source.primaries)
                && [1, 6].contains(&source.transfer)
                && source.range != SignalRange::Unspecified
                && source.chroma == ChromaSiting::Center)
    }
}

/// Pure YUVA resizing keeps signal encoding; sws_scale does not copy frame tags.
pub(in crate::ffmpeg) fn copy_color_properties(source: &frame::Video, target: &mut frame::Video) {
    target.set_color_space(source.color_space());
    target.set_color_range(source.color_range());
    target.set_color_primaries(source.color_primaries());
    target.set_color_transfer_characteristic(source.color_transfer_characteristic());
    // Both frames are owned/borrowed for the duration of this metadata update.
    // ffmpeg-next exposes the chroma-location getter but no matching setter.
    unsafe {
        (*target.as_mut_ptr()).chroma_location = (*source.as_ptr()).chroma_location;
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::util::color::{Primaries, Space, TransferCharacteristic};

    use super::*;

    #[test]
    fn descriptors_preserve_padded_planes_visible_bounds_and_explicit_tags() {
        let mut frame = frame::Video::new(Pixel::YUVA420P, 18, 10);
        frame.set_color_space(Space::BT709);
        frame.set_color_range(Range::MPEG);
        frame.set_color_primaries(Primaries::BT709);
        frame.set_color_transfer_characteristic(TransferCharacteristic::BT709);
        let bounds = OverlayBounds {
            x: 2,
            y: 2,
            width: 8,
            height: 4,
        };
        let descriptor = yuva_descriptor(&frame, bounds, 45, ColorEncoding::Bt709Limited).unwrap();
        let metadata = descriptor.metadata.unwrap();
        assert_eq!(metadata.planes[0].unwrap().stride_bytes, frame.stride(0));
        assert_eq!(metadata.planes[1].unwrap().width, 9);
        assert_eq!(metadata.planes[3].unwrap().height, 10);
        assert_eq!(metadata.visible.width, 8);
        assert_eq!(
            (
                metadata.color.matrix,
                metadata.color.primaries,
                metadata.color.transfer
            ),
            (1, 1, 1)
        );
        assert_eq!(metadata.color.range, SignalRange::Limited);
        assert!(yuva_descriptor(&frame, bounds, 45, ColorEncoding::Bt601Limited).is_err());
        frame.set_color_range(Range::JPEG);
        assert!(yuva_descriptor(&frame, bounds, 45, ColorEncoding::Bt709Limited).is_err());
        let mut invalid = descriptor;
        invalid.metadata.as_mut().unwrap().planes[0]
            .as_mut()
            .unwrap()
            .stride_bytes = 1;
        assert!(invalid.validate().is_err());
        invalid = descriptor;
        invalid.metadata.as_mut().unwrap().visible.x = 18;
        assert!(invalid.validate().is_err());
        invalid = descriptor;
        invalid.alpha = Alpha::PremultipliedEncoded;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn signal_policy_never_assumes_unknown_metadata_or_silently_converts() {
        let frame = frame::Video::new(Pixel::YUVA420P, 4, 4);
        let unknown = color_properties(&frame);
        assert!(!ColorPolicy::MatchingSignal.supports(unknown, unknown));
        let known = ColorProperties {
            matrix: 1,
            primaries: 1,
            transfer: 1,
            range: SignalRange::Limited,
            chroma: ChromaSiting::Center,
        };
        assert!(ColorPolicy::MatchingSignal.supports(known, known));
        for incompatible in [
            ColorProperties {
                range: SignalRange::Full,
                ..known
            },
            ColorProperties { matrix: 6, ..known },
            ColorProperties {
                chroma: ChromaSiting::Left,
                ..known
            },
        ] {
            assert!(!ColorPolicy::MatchingSignal.supports(known, incompatible));
            assert!(ColorPolicy::LegacySignal.supports(known, incompatible));
        }
    }

    #[test]
    fn resizing_preserves_color_tags_without_touching_pixels() {
        let mut source = frame::Video::new(Pixel::YUVA420P, 16, 16);
        source.set_color_space(Space::BT709);
        source.set_color_range(Range::JPEG);
        source.set_color_primaries(Primaries::BT709);
        let mut target = frame::Video::new(Pixel::YUVA420P, 8, 8);
        target.data_mut(0).fill(70);
        copy_color_properties(&source, &mut target);
        assert_eq!(color_properties(&source), color_properties(&target));
        assert!(target.data(0).iter().all(|&value| value == 70));
    }
}
