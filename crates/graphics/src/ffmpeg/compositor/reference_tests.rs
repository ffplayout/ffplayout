//! Independent f64 signal-domain oracle. This deliberately does not call the
//! production blend, clipping, alpha-reduction or integer-rounding helpers.
use ffmpeg_next::{
    format::Pixel,
    frame,
    util::color::{Range, Space},
};
use rayon::ThreadPoolBuilder;

use crate::blend::{Plane, PlaneMut, blend_plane};

use super::overlay::{
    BlendExecution, OverlayFrame, alpha_bounds, blend_overlay_with_execution, chroma_alpha,
};

#[derive(Clone, Copy, Debug)]
enum SignalRange {
    Limited,
    Full,
}

impl SignalRange {
    fn encoding(self, plane: usize) -> (f64, f64) {
        match (self, plane) {
            (Self::Limited, 0) => (16.0, 219.0),
            (Self::Limited, _) => (128.0, 224.0),
            (Self::Full, 0) => (0.0, 255.0),
            (Self::Full, _) => (128.0, 255.0),
        }
    }

    fn tag(self) -> Range {
        match self {
            Self::Limited => Range::MPEG,
            Self::Full => Range::JPEG,
        }
    }
}

/// BT.709 encoded RGB -> encoded YCbCr. No transfer-function conversion: this
/// oracle tests the existing signal-domain policy, not linear-light blending.
fn color(rgb: [f64; 3], range: SignalRange) -> [u8; 3] {
    let [r, g, b] = rgb;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let components = [y, (b - y) / 1.8556, (r - y) / 1.5748];

    std::array::from_fn(|plane| {
        let (offset, scale) = range.encoding(plane);

        (offset + scale * components[plane])
            .round()
            .clamp(0.0, 255.0) as u8
    })
}

fn video(width: u32, height: u32, samples: [u8; 3], range: SignalRange) -> frame::Video {
    let mut video = frame::Video::new(Pixel::YUV420P, width, height);
    video.set_color_space(Space::BT709);
    video.set_color_range(range.tag());

    for (plane, sample) in samples.into_iter().enumerate() {
        video.data_mut(plane).fill(173); // Padding is a canary, not image content.
        let stride = video.stride(plane);
        let width = video.plane_width(plane) as usize;

        for row in 0..video.plane_height(plane) as usize {
            video.data_mut(plane)[row * stride..row * stride + width].fill(sample);
        }
    }

    video
}

fn overlay(range: SignalRange, rgb: [f64; 3], alpha: impl Fn(usize, usize) -> u8) -> OverlayFrame {
    let (width, height) = (50, 18);
    let mut source = frame::Video::new(Pixel::YUVA420P, width, height);
    source.set_color_space(Space::BT709);
    source.set_color_range(range.tag());

    for (plane, sample) in color(rgb, range).into_iter().enumerate() {
        source.data_mut(plane).fill(sample);
    }

    source.data_mut(3).fill(0);
    let stride = source.stride(3);

    for y in 0..height as usize {
        for x in 0..width as usize {
            source.data_mut(3)[y * stride + x] = alpha(x, y);
        }
    }

    OverlayFrame {
        lease: None,
        bounds: alpha_bounds(&source),
        chroma_alpha: chroma_alpha(&source, width, height),
        frame: source,
        x: 0,
        y: 0,
        width,
        height,
        opacity: 255,
    }
}

/// Retain f64 results between layers instead of reproducing production's byte
/// quantization. Average full-resolution coverage directly for each chroma cell.
/// Source chroma is uniform inside a cell; RGBA preparation/color conversion and
/// arbitrary chroma siting are explicitly outside this test's input contract.
fn reference(
    expected: &mut [Vec<f64>; 3],
    target: &frame::Video,
    source: &OverlayFrame,
    factor: f64,
    range: SignalRange,
) {
    assert_eq!(source.x % 2, 0);
    assert_eq!(source.y % 2, 0);

    for (plane, values) in expected.iter_mut().enumerate() {
        let divisor = if plane == 0 { 1 } else { 2 };
        let (offset, scale) = range.encoding(plane);

        for y in 0..target.plane_height(plane) as usize {
            for x in 0..target.plane_width(plane) as usize {
                let sx = x as i64 * divisor - i64::from(source.x);
                let sy = y as i64 * divisor - i64::from(source.y);

                if sx < 0
                    || sy < 0
                    || sx >= i64::from(source.width)
                    || sy >= i64::from(source.height)
                {
                    continue;
                }

                let mut coverage = 0.0;

                for dy in 0..divisor as usize {
                    for dx in 0..divisor as usize {
                        coverage += f64::from(
                            source.frame.data(3)
                                [(sy as usize + dy) * source.frame.stride(3) + sx as usize + dx],
                        ) / 255.0;
                    }
                }

                let alpha = coverage / (divisor * divisor) as f64 * f64::from(source.opacity)
                    / 255.0
                    * factor;
                let index = y * target.stride(plane) + x;
                let foreground = f64::from(
                    source.frame.data(plane)[sy as usize / divisor as usize
                        * source.frame.stride(plane)
                        + sx as usize / divisor as usize],
                );
                let background_signal = (values[index] - offset) / scale;
                let foreground_signal = (foreground - offset) / scale;
                values[index] = offset
                    + scale * (foreground_signal * alpha + background_signal * (1.0 - alpha));
            }
        }
    }
}

fn compare(
    actual: &frame::Video,
    expected: &[Vec<f64>; 3],
    layers: usize,
    exact: bool,
) -> [f64; 3] {
    let mut maximum = [0.0_f64; 3];
    for (plane, expected) in expected.iter().enumerate() {
        // Per layer: <=0.5 sample rounding, <=0.5 effective-alpha rounding,
        // <=0.5 presentation-opacity rounding; chroma also floors coverage (<1).
        // Worst case contrast is 255. Independent f64 layers accumulate no byte rounding.
        let tolerance = if exact {
            0.0
        } else if plane == 0 {
            2.0
        } else {
            3.0
        } * layers as f64;
        let stride = actual.stride(plane);
        let width = actual.plane_width(plane) as usize;

        for (index, (&actual, &expected)) in actual.data(plane).iter().zip(expected).enumerate() {
            let padding = index % stride >= width;
            let limit = if padding { 0.0 } else { tolerance };
            let error = (f64::from(actual) - expected).abs();
            maximum[plane] = maximum[plane].max(error);
            assert!(
                error <= limit + 1e-9,
                "plane={plane}, byte={index}, actual={actual}, reference={expected}, tolerance={limit}"
            );
        }
    }

    maximum
}

#[test]
fn all_byte_pairs_match_independent_source_over_with_one_sample_quantization_bound() {
    let width = 256;
    let source: Vec<_> = (0..256).flat_map(|_| 0..=255u8).collect();
    let background: Vec<_> = (0..=255u8).flat_map(|value| vec![value; width]).collect();

    for alpha in [0, 1, 63, 127, 128, 192, 254, 255] {
        for opacity in [0, 1, 64, 127, 179, 254, 255] {
            let mut actual = background.clone();
            let coverage = vec![alpha; actual.len()];
            blend_plane(
                PlaneMut {
                    data: &mut actual,
                    stride: width,
                    x: 0,
                    y: 0,
                },
                Plane {
                    data: &source,
                    stride: width,
                    x: 0,
                    y: 0,
                },
                Plane {
                    data: &coverage,
                    stride: width,
                    x: 0,
                    y: 0,
                },
                width,
                width,
                opacity,
            );
            let a = f64::from(alpha) / 255.0 * f64::from(opacity) / 255.0;

            for ((actual, source), background) in actual.iter().zip(&source).zip(&background) {
                let expected = f64::from(*source) * a + f64::from(*background) * (1.0 - a);
                assert!((f64::from(*actual) - expected).abs() <= 1.0 + 1e-9);

                if alpha == 0 || opacity == 0 || (alpha == 255 && opacity == 255) {
                    assert_eq!(f64::from(*actual), expected);
                }
            }
        }
    }
}

#[test]
fn transparent_and_opaque_endpoints_preserve_limited_and_full_range_signal_and_padding() {
    for range in [SignalRange::Limited, SignalRange::Full] {
        for alpha in [0, 255] {
            let source = overlay(range, [1.0, 0.0, 0.0], |_, _| alpha);
            let mut actual = video(64, 32, color([0.0, 0.0, 1.0], range), range);
            let mut expected =
                std::array::from_fn(|p| actual.data(p).iter().map(|&v| f64::from(v)).collect());
            reference(&mut expected, &actual, &source, 1.0, range);
            blend_overlay_with_execution(&mut actual, source.as_ref(), 1.0, BlendExecution::Serial);
            compare(&actual, &expected, 1, true);
        }
    }
}

#[test]
fn saturated_edges_gradients_and_overlapping_layers_match_signal_reference_in_all_execution_modes()
{
    let pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();
    let mut maximum = [0.0_f64; 3];

    for range in [SignalRange::Limited, SignalRange::Full] {
        for placement in [(-8, -4), (0, 0), (22, 20)] {
            for factor in [0.0, 0.37, 1.0] {
                let mut red = overlay(range, [1.0, 0.0, 0.0], |x, y| {
                    if x < 2 || y < 2 || x >= 48 || y >= 16 {
                        0
                    } else {
                        ((x * 17 + y * 43) % 256) as u8
                    }
                });
                red.x = placement.0;
                red.y = placement.1;
                red.opacity = 179;
                let mut green = overlay(range, [0.0, 1.0, 0.0], |x, y| {
                    ((x * 255 / 49 + y * 7) % 256) as u8
                });
                green.x = 6;
                green.y = 2;
                green.opacity = 213;

                for execution in [
                    BlendExecution::Serial,
                    BlendExecution::Parallel(&pool),
                    BlendExecution::Adaptive,
                ] {
                    let mut actual = video(64, 32, color([0.0, 0.0, 1.0], range), range);
                    let mut expected = std::array::from_fn(|p| {
                        actual.data(p).iter().map(|&v| f64::from(v)).collect()
                    });
                    reference(&mut expected, &actual, &red, factor, range);
                    blend_overlay_with_execution(&mut actual, red.as_ref(), factor, execution);
                    compare(&actual, &expected, 1, false);
                    reference(&mut expected, &actual, &green, 0.63, range);
                    blend_overlay_with_execution(
                        &mut actual,
                        green.as_ref(),
                        0.63,
                        BlendExecution::Serial,
                    );
                    let error = compare(&actual, &expected, 2, false);

                    for plane in 0..3 {
                        maximum[plane] = maximum[plane].max(error[plane]);
                    }

                    assert_eq!(actual.color_range(), range.tag());
                    assert_eq!(actual.color_space(), Space::BT709);
                }
            }
        }
    }
    eprintln!("Two-layer maximum absolute error [Y, U, V] in byte levels: {maximum:.6?}");
}

#[test]
fn reference_bt709_vectors_have_the_expected_signal_offsets_and_primaries() {
    assert_eq!(color([0.0; 3], SignalRange::Limited), [16, 128, 128]);
    assert_eq!(color([1.0; 3], SignalRange::Limited), [235, 128, 128]);
    assert_eq!(color([0.0; 3], SignalRange::Full), [0, 128, 128]);
    assert_eq!(color([1.0; 3], SignalRange::Full), [255, 128, 128]);
    assert_eq!(color([1.0, 0.0, 0.0], SignalRange::Limited), [63, 102, 240]);
    assert_eq!(color([0.0, 0.0, 1.0], SignalRange::Limited), [32, 240, 118]);
}

#[test]
fn one_opaque_luma_sample_covers_one_quarter_of_its_chroma_cell_without_leaking_to_neighbors() {
    for range in [SignalRange::Limited, SignalRange::Full] {
        for sample in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let source = overlay(range, [1.0, 0.0, 0.0], |x, y| {
                if (x, y) == sample { 255 } else { 0 }
            });
            let background = color([0.0, 0.0, 1.0], range);
            let mut actual = video(64, 32, background, range);
            let mut expected =
                std::array::from_fn(|p| actual.data(p).iter().map(|&v| f64::from(v)).collect());
            reference(&mut expected, &actual, &source, 1.0, range);
            blend_overlay_with_execution(&mut actual, source.as_ref(), 1.0, BlendExecution::Serial);
            compare(&actual, &expected, 1, false);

            for (plane, background) in background.into_iter().enumerate() {
                for y in 0..actual.plane_height(plane) as usize {
                    for x in 0..actual.plane_width(plane) as usize {
                        let covered = if plane == 0 {
                            (x, y) == sample
                        } else {
                            x == 0 && y == 0
                        };

                        if !covered {
                            assert_eq!(
                                actual.data(plane)[y * actual.stride(plane) + x],
                                background
                            );
                        }
                    }
                }
            }
        }
    }
}
