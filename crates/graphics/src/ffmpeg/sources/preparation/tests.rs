use ffmpeg_next::{
    frame,
    util::{
        color::{Range, Space},
        format::pixel::Pixel,
    },
};

use crate::surface::Alpha;

use super::SourcePreparation;

fn source(width: u32, height: u32, associated: bool) -> frame::Video {
    let mut frame = frame::Video::new(Pixel::RGBA64LE, width, height);
    frame.set_color_space(Space::RGB);
    frame.set_color_range(Range::JPEG);
    let stride = frame.stride(0);

    for y in 0..height as usize {
        for x in 0..width as usize {
            let alpha = [0, 1, 257, 8192, 32768, 65535][x % 6];
            let color = [49151_u16, 16384, 32768, alpha];

            for (channel, value) in color.into_iter().enumerate() {
                let value = if associated && channel < 3 {
                    ((u64::from(value) * u64::from(alpha) + 32767) / 65535) as u16
                } else {
                    value
                };
                frame.data_mut(0)
                    [y * stride + x * 8 + channel * 2..y * stride + x * 8 + channel * 2 + 2]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
    }

    frame
}

#[test]
fn unassociation_handles_zero_and_very_small_alpha_without_nonfinite_or_overflow() {
    let input = source(6, 2, true);
    let frame = SourcePreparation::default()
        .prepare(&input, None, Alpha::PremultipliedEncoded, (6, 2))
        .unwrap();
    assert_eq!(frame.data(0)[0], 16);
    assert_eq!(frame.data(3)[0], 0);
    // At alpha=1/65535 the encoded color has unavoidable source quantization,
    // but must remain finite/clamped. [1,0,1]/1 is saturated magenta.
    assert!(frame.data(0)[1].abs_diff(106) <= 1);
    assert!(frame.data(0)[4].abs_diff(110) <= 1);
    assert_eq!(frame.data(3)[4], 128);
}

#[test]
fn straight_and_encoded_premultiplied_scaling_produce_equivalent_composites() {
    let mut prep = SourcePreparation::default();
    let straight = source(12, 4, false);
    let associated = source(12, 4, true);
    let left = prep
        .prepare(&straight, None, Alpha::Straight, (18, 6))
        .unwrap();
    let right = prep
        .prepare(&associated, None, Alpha::PremultipliedEncoded, (18, 6))
        .unwrap();

    for plane in 0..4 {
        for row in 0..left.plane_height(plane) as usize {
            for column in 0..left.plane_width(plane) as usize {
                let a = left.data(plane)[row * left.stride(plane) + column];
                let b = right.data(plane)[row * right.stride(plane) + column];
                assert!(
                    a.abs_diff(b) <= 1,
                    "plane {plane}, {column}:{row}: {a} vs {b}"
                );
            }
        }
    }
}

#[test]
fn hidden_saturated_rgb_does_not_bleed_into_scaled_visible_edges() {
    let mut input = frame::Video::new(Pixel::RGBA64LE, 4, 2);
    input.set_color_space(Space::RGB);
    let stride = input.stride(0);

    for y in 0..2 {
        for x in 0..4 {
            let rgba = if x < 2 {
                [65535_u16, 0, 0, 65535]
            } else {
                [0, 0, 65535, 0]
            };

            for (channel, value) in rgba.into_iter().enumerate() {
                input.data_mut(0)
                    [y * stride + x * 8 + channel * 2..y * stride + x * 8 + channel * 2 + 2]
                    .copy_from_slice(&value.to_le_bytes());
            }
        }
    }

    let output = SourcePreparation::default()
        .prepare(&input, None, Alpha::Straight, (8, 4))
        .unwrap();
    // Independent BT.601 red reference. Hidden blue must not tint the transition.
    for x in 0..4 {
        assert!(output.data(1)[x].abs_diff(90) <= 1 || output.data(3)[x * 2] == 0);
        assert!(output.data(2)[x].abs_diff(240) <= 1 || output.data(3)[x * 2] == 0);
    }
}

#[test]
fn encoded_rgb_unassociation_does_not_divide_yuv_offsets() {
    let mut source = frame::Video::new(Pixel::YUVA444P, 2, 2);
    source.set_color_space(Space::BT709);
    source.set_color_range(Range::MPEG);
    source.data_mut(0).fill(16);
    source.data_mut(1).fill(128);
    source.data_mut(2).fill(128);
    source.data_mut(3).fill(128);
    let output = SourcePreparation::default()
        .prepare(&source, None, Alpha::PremultipliedEncoded, (2, 2))
        .unwrap();
    assert_eq!(output.data(0)[0], 16);
    assert_eq!(output.data(1)[0], 128);
    assert_eq!(output.data(2)[0], 128);
    assert_eq!(output.data(3)[0], 128);
    assert_eq!(output.color_space(), Space::BT709);
    assert_eq!(output.color_range(), Range::MPEG);
}

#[test]
fn compatible_unscaled_sources_keep_legacy_pixels_without_rgb_normalization() {
    use ffmpeg_next::software::scaling;

    for format in [Pixel::RGBA64LE, Pixel::YUVA420P, Pixel::RGB24] {
        let mut input = frame::Video::new(format, 8, 4);

        for plane in 0..input.planes() {
            input.data_mut(plane).fill(100 + plane as u8 * 20);
        }

        let mut legacy = scaling::Context::get(
            format,
            8,
            4,
            Pixel::YUVA420P,
            8,
            4,
            scaling::flag::Flags::BILINEAR,
        )
        .unwrap();
        let mut expected = frame::Video::empty();
        legacy.run(&input, &mut expected).unwrap();
        let actual = SourcePreparation::default()
            .prepare(&input, None, Alpha::Straight, (8, 4))
            .unwrap();

        for plane in 0..4 {
            for row in 0..expected.plane_height(plane) as usize {
                let width = expected.plane_width(plane) as usize;
                assert_eq!(
                    &actual.data(plane)
                        [row * actual.stride(plane)..row * actual.stride(plane) + width],
                    &expected.data(plane)
                        [row * expected.stride(plane)..row * expected.stride(plane) + width]
                );
            }
        }
    }
}

#[test]
fn nonuniform_chroma_and_soft_alpha_match_an_independent_rgb_reference() {
    let mut input = frame::Video::new(Pixel::RGBA64LE, 2, 2);
    input.set_color_space(Space::RGB);
    let colors = [
        [1.0_f64, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.8, 0.4, 0.2],
    ];
    let alphas = [0_u16, 4096, 32768, 65535];
    let stride = input.stride(0);
    let mut weighted = [0.0; 3];
    let mut coverage = 0.0;

    for (index, (color, alpha)) in colors.into_iter().zip(alphas).enumerate() {
        let offset = (index / 2) * stride + (index % 2) * 8;

        for (channel, value) in color.into_iter().enumerate() {
            let premultiplied = (value * f64::from(alpha)).round() as u16;
            input.data_mut(0)[offset + channel * 2..offset + channel * 2 + 2]
                .copy_from_slice(&premultiplied.to_le_bytes());
            weighted[channel] += value * f64::from(alpha);
        }

        input.data_mut(0)[offset + 6..offset + 8].copy_from_slice(&alpha.to_le_bytes());
        coverage += f64::from(alpha);
    }

    let output = SourcePreparation::default()
        .prepare(&input, None, Alpha::PremultipliedEncoded, (2, 2))
        .unwrap();
    let mean = weighted.map(|value| value / coverage);
    let luma = 0.299 * mean[0] + 0.587 * mean[1] + 0.114 * mean[2];
    let cb = (128.0 + 224.0 * (mean[2] - luma) / 1.772).round() as u8;
    let cr = (128.0 + 224.0 * (mean[0] - luma) / 1.402).round() as u8;
    assert!(output.data(1)[0].abs_diff(cb) <= 1);
    assert!(output.data(2)[0].abs_diff(cr) <= 1);

    for (index, (rgb, alpha)) in colors.into_iter().zip(alphas).enumerate() {
        let x = index % 2;
        let y = index / 2;
        let expected = if alpha == 0 {
            16
        } else {
            (16.0 + 219.0 * (0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2])).round() as u8
        };
        assert!(output.data(0)[y * output.stride(0) + x].abs_diff(expected) <= 1);
        assert_eq!(
            output.data(3)[y * output.stride(3) + x],
            ((u32::from(alpha) + 128) / 257) as u8
        );
    }
}

#[test]
fn auxiliary_masks_must_be_monochrome_and_match_source_dimensions() {
    let input = source(4, 2, false);
    let color_mask = frame::Video::new(Pixel::RGBA, 4, 2);
    let wrong_size = frame::Video::new(Pixel::GRAY8, 2, 2);
    let mut preparation = SourcePreparation::default();
    assert!(
        preparation
            .prepare(&input, Some(&color_mask), Alpha::Straight, (4, 2))
            .is_err()
    );
    assert!(
        preparation
            .prepare(&input, Some(&wrong_size), Alpha::Straight, (4, 2))
            .is_err()
    );
}

#[cfg(ff_graphics_av_alpha_mode)]
#[test]
fn decoder_alpha_metadata_is_read_and_prepared_frames_are_straight() {
    use ffmpeg_next::{ffi::AVAlphaMode, util::color::TransferCharacteristic};

    let mut input = source(4, 2, true);
    // SAFETY: input exclusively owns this AVFrame.
    unsafe {
        (*input.as_mut_ptr()).alpha_mode = AVAlphaMode::AVALPHA_MODE_PREMULTIPLIED;
    }
    assert_eq!(
        super::frame_alpha(&input),
        Some(Alpha::PremultipliedEncoded)
    );
    let output = SourcePreparation::default()
        .prepare(&input, None, Alpha::PremultipliedEncoded, (4, 2))
        .unwrap();
    assert_eq!(super::frame_alpha(&output), Some(Alpha::Straight));
    input.set_color_transfer_characteristic(TransferCharacteristic::Linear);
    assert_eq!(super::frame_alpha(&input), Some(Alpha::PremultipliedLinear));
    assert!(
        SourcePreparation::default()
            .prepare(&input, None, Alpha::PremultipliedLinear, (4, 2))
            .is_err()
    );
    // SAFETY: input remains exclusively owned.
    unsafe {
        (*input.as_mut_ptr()).alpha_mode = AVAlphaMode::AVALPHA_MODE_UNSPECIFIED;
    }
    assert_eq!(super::frame_alpha(&input), None);
}

#[test]
fn full_range_black_and_centered_output_chroma_have_correct_metadata() {
    let mut source = frame::Video::new(Pixel::YUVA444P, 2, 2);
    source.set_color_space(Space::BT709);
    source.set_color_range(Range::JPEG);
    source.data_mut(0).fill(0);
    source.data_mut(1).fill(128);
    source.data_mut(2).fill(128);
    source.data_mut(3).fill(128);
    let output = SourcePreparation::default()
        .prepare(&source, None, Alpha::PremultipliedEncoded, (2, 2))
        .unwrap();
    assert_eq!(output.data(0)[0], 0);
    assert_eq!(output.data(1)[0], 128);
    assert_eq!(output.data(2)[0], 128);
    assert_eq!(output.color_range(), Range::JPEG);
    assert_eq!(
        output.chroma_location(),
        ffmpeg_next::util::chroma::Location::Center
    );
}

#[test]
fn simd_quantization_matches_f64_reference_for_tails_strides_matrices_and_extremes() {
    use super::{color::ColorConversion, rgba};

    let mut seed = 0x7aba_8c9d_u32;

    for width in [2, 6, 8, 10, 18, 30, 32, 34, 66] {
        let mut input = frame::Video::new(Pixel::RGBA64LE, width, 6);
        let stride = input.stride(0);

        for row in 0..6 {
            for pixel in input.data_mut(0)[row * stride..row * stride + width as usize * 8]
                .as_chunks_mut::<8>()
                .0
            {
                for channel in pixel.as_chunks_mut::<2>().0 {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    channel.copy_from_slice(&(seed as u16).to_le_bytes());
                }
            }
        }

        for (kr, kb) in [
            (0.299, 0.114),
            (0.2126, 0.0722),
            (0.30, 0.11),
            (0.212, 0.087),
            (0.2627, 0.0593),
        ] {
            for full in [false, true] {
                let mut conversion = ColorConversion::from_frame(&input).unwrap();
                conversion.kr = kr;
                conversion.kb = kb;
                conversion.full = full;
                let reference = rgba::to_yuva_reference(&input, &input, conversion).unwrap();
                let simd = rgba::to_yuva(&input, &input, conversion, None).unwrap();

                for plane in 0..4 {
                    for row in 0..simd.plane_height(plane) as usize {
                        for x in 0..simd.plane_width(plane) as usize {
                            let expected = reference.data(plane)[row * reference.stride(plane) + x];
                            let actual = simd.data(plane)[row * simd.stride(plane) + x];
                            assert!(
                                expected.abs_diff(actual) <= 1,
                                "width={width} plane={plane} {x}:{row} expected={expected} actual={actual}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn native_and_portable_simd_rows_have_equivalent_visible_output() {
    use super::{color::ColorConversion, kernels::Rows};

    for width in [2, 6, 8, 10, 18, 30, 66] {
        let input = source(width, 2, true);
        let mut native = [
            vec![0_u8; width as usize * 2],
            vec![0; width as usize / 2],
            vec![0; width as usize / 2],
            vec![0; width as usize * 2],
        ];
        let mut portable = native.clone();
        let mut generic = native.clone();
        let conversion = ColorConversion::from_frame(&input).unwrap();

        for (buffers, runner) in [(&mut native, 0), (&mut portable, 1), (&mut generic, 2)] {
            let [y, u, v, alpha] = buffers;
            let rows = Rows {
                pixels: input.data(0),
                source_stride: input.stride(0),
                width: width as usize,
                y,
                y_stride: width as usize,
                u,
                v,
                alpha,
                alpha_stride: width as usize,
                color: conversion,
            };

            match runner {
                0 => rows.run(),
                1 => rows.portable(),
                _ => rten_simd::SimdOp::eval(rows, rten_simd::isa::GenericIsa::new()),
            }
        }

        for candidate in [&portable, &generic] {
            for (a, b) in native.iter().zip(candidate) {
                assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 1));
            }
        }
    }
}

#[test]
fn simd_association_keeps_exact_sixteen_bit_alpha_and_rounding() {
    use super::{resize::RgbaResizer, rgba};

    for alpha in [0_u16, 1, 2, 127, 255, 256, 32767, 32768, 65534, 65535] {
        let mut input = frame::Video::new(Pixel::RGBA64LE, 256, 256);
        let stride = input.stride(0);

        for row in 0..256 {
            for (x, pixel) in input.data_mut(0)[row * stride..row * stride + 256 * 8]
                .as_chunks_mut::<8>()
                .0
                .iter_mut()
                .enumerate()
            {
                let color = (row * 256 + x) as u16;

                for (channel, value) in [color, 65535 - color, color / 2, alpha]
                    .into_iter()
                    .enumerate()
                {
                    pixel[channel * 2..channel * 2 + 2].copy_from_slice(&value.to_le_bytes());
                }
            }
        }

        let mut reference = frame::Video::new(Pixel::RGBA64LE, 256, 256);
        reference.data_mut(0).copy_from_slice(input.data(0));
        rgba::associate(&mut reference);
        RgbaResizer::default().associate(&mut input, None).unwrap();
        assert_eq!(reference.data(0), input.data(0), "alpha={alpha}");
    }
}

#[test]
fn parallel_resize_keeps_filter_halos_phase_and_bounded_reused_storage() {
    use super::PreparationExecution;

    for threads in [1, 4] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();

        for (input_size, output_size) in [
            ((13, 9), (22, 14)),
            ((46, 30), (18, 10)),
            ((18, 14), (66, 34)),
            ((64, 36), (32, 18)),
        ] {
            let mut input = source(input_size.0, input_size.1, false);
            let stride = input.stride(0);

            // Change both color and coverage vertically so an incorrect band
            // crop or missing filter halo cannot hide behind identical rows.
            for y in 0..input_size.1 as usize {
                for (x, pixel) in input.data_mut(0)
                    [y * stride..y * stride + input_size.0 as usize * 8]
                    .as_chunks_mut::<8>()
                    .0
                    .iter_mut()
                    .enumerate()
                {
                    for (channel, multiplier) in [1931, 4567, 137, 7919].into_iter().enumerate() {
                        let value = ((x * 6301 + y * multiplier + channel * 9337) % 65536) as u16;
                        pixel[channel * 2..channel * 2 + 2].copy_from_slice(&value.to_le_bytes());
                    }
                }
            }

            let mut serial_preparation = SourcePreparation::default();
            let mut parallel_preparation = SourcePreparation::default();
            let serial = serial_preparation
                .prepare_with_execution(
                    &input,
                    None,
                    Alpha::Straight,
                    output_size,
                    PreparationExecution::Serial,
                )
                .unwrap();
            let mut storage = None;

            for _ in 0..8 {
                let parallel = parallel_preparation
                    .prepare_with_execution(
                        &input,
                        None,
                        Alpha::Straight,
                        output_size,
                        PreparationExecution::Parallel(&pool),
                    )
                    .unwrap();

                for plane in 0..4 {
                    for row in 0..parallel.plane_height(plane) as usize {
                        for x in 0..parallel.plane_width(plane) as usize {
                            let a = serial.data(plane)[row * serial.stride(plane) + x];
                            let b = parallel.data(plane)[row * parallel.stride(plane) + x];
                            assert!(
                                a.abs_diff(b) <= 1,
                                "threads={threads} plane={plane} at {x}:{row}: {a} != {b}"
                            );
                        }
                    }
                }

                let current = parallel_preparation.optimized.storage_bytes();
                assert_eq!(
                    *storage.get_or_insert(current),
                    current,
                    "resizer scratch must not grow per frame"
                );
            }

            parallel_preparation
                .prepare(
                    &input,
                    None,
                    Alpha::Straight,
                    (
                        input_size.0 + input_size.0 % 2,
                        input_size.1 + input_size.1 % 2,
                    ),
                )
                .unwrap();
            if input_size.0.is_multiple_of(2) && input_size.1.is_multiple_of(2) {
                assert_eq!(
                    parallel_preparation.optimized.storage_bytes(),
                    0,
                    "geometry changes release old scratch"
                );
            }
        }
    }
}

#[test]
fn associated_rgb16_borrowing_never_modifies_the_decoder_frame() {
    use super::PreparationExecution;

    let input = source(18, 10, true);
    let original = input.data(0).to_vec();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let mut preparation = SourcePreparation::default();

    for size in [(18, 10), (36, 20), (18, 10)] {
        preparation
            .prepare_with_execution(
                &input,
                None,
                Alpha::PremultipliedEncoded,
                size,
                PreparationExecution::Parallel(&pool),
            )
            .unwrap();
        assert_eq!(input.data(0), original);
    }
}

#[test]
fn rgb_depth_expansion_is_exact_for_all_bytes_and_planar_channels() {
    use super::rgb;

    for width in [2, 6, 10, 18, 258] {
        let mut input = frame::Video::new(Pixel::RGBA, width, 2);
        let stride = input.stride(0);

        for row in 0..2 {
            for (index, value) in input.data_mut(0)[row * stride..row * stride + width as usize * 4]
                .iter_mut()
                .enumerate()
            {
                *value = (index + row) as u8;
            }
        }

        let output = rgb::convert(&input).unwrap();

        for row in 0..2 {
            for index in 0..width as usize * 4 {
                let source = input.data(0)[row * stride + index];
                assert_eq!(
                    &output.data(0)[row * output.stride(0) + index * 2
                        ..row * output.stride(0) + index * 2 + 2],
                    &[source; 2]
                );
            }
        }
    }

    let mut planar = frame::Video::new(Pixel::GBRP, 6, 2);
    planar.data_mut(0).fill(70);
    planar.data_mut(1).fill(210);
    planar.data_mut(2).fill(15);
    let output = rgb::convert(&planar).unwrap();
    assert_eq!(&output.data(0)[..8], &[15, 15, 70, 70, 210, 210, 255, 255]);
}

#[test]
fn standard_scaling_matches_legacy_but_never_skips_premultiplied_normalization() {
    use super::PreparationExecution;

    let input = source(12, 4, false);
    let size = (6, 2);
    let standard = SourcePreparation::default()
        .prepare_with_quality(&input, None, Alpha::Straight, size, false)
        .unwrap();
    let legacy = SourcePreparation::default()
        .direct
        .run(&input, Pixel::YUVA420P, size, None)
        .unwrap();

    for plane in 0..4 {
        for row in 0..standard.plane_height(plane) as usize {
            let width = standard.plane_width(plane) as usize;
            assert_eq!(
                &standard.data(plane)[row * standard.stride(plane)..][..width],
                &legacy.data(plane)[row * legacy.stride(plane)..][..width],
            );
        }
    }

    let associated = source(12, 4, true);
    let standard = SourcePreparation::default()
        .prepare_with_quality(&associated, None, Alpha::PremultipliedEncoded, size, false)
        .unwrap();
    let reference = SourcePreparation::default()
        .prepare_with_execution(
            &associated,
            None,
            Alpha::PremultipliedEncoded,
            size,
            PreparationExecution::Serial,
        )
        .unwrap();

    for plane in 0..4 {
        for row in 0..standard.plane_height(plane) as usize {
            let width = standard.plane_width(plane) as usize;
            assert_eq!(
                &standard.data(plane)[row * standard.stride(plane)..][..width],
                &reference.data(plane)[row * reference.stride(plane)..][..width],
            );
        }
    }
}
