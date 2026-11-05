use std::path::{Path, PathBuf};

use ffmpeg_next::{codec, format, frame, util::format::pixel::Pixel};

use crate::{
    ffmpeg::sources::logo::{LogoOverlay, blend_logo},
    style::LogoStyle,
};

use super::LogoDecoder;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests_assets/storage/media_images")
        .join(name)
}

fn style() -> LogoStyle {
    LogoStyle {
        scale: None,
        opacity: 1.0,
        position: "0:0".into(),
    }
}

fn software_av1_available() -> bool {
    let available = codec::decoder::find_by_name("libdav1d").is_some()
        || codec::decoder::find_by_name("libaom-av1").is_some();

    if !available {
        eprintln!("AVIF decode regression requires an FFmpeg build with libdav1d or libaom-av1");
    }

    available
}

fn assert_alpha(surface: &LogoOverlay, expected: [u8; 3]) {
    for (x, expected) in [4, 16, 28].into_iter().zip(expected) {
        assert!(
            surface.frame.data(3)[x].abs_diff(expected) <= 1,
            "alpha at x={x}: {} != {expected}",
            surface.frame.data(3)[x]
        );
    }
}

#[test]
fn avif_stills_merge_eight_and_ten_bit_alpha_including_static_logo_loading() {
    if !software_av1_available() {
        return;
    }

    for name in ["logo-alpha.avif", "logo-alpha-10bit.avif"] {
        let path = fixture(name);
        let mut decoder = LogoDecoder::new(format::input(&path).unwrap(), style(), 64, 64).unwrap();
        assert!(decoder.alpha.is_some());
        let source = decoder.next_frame().unwrap().unwrap();
        assert_alpha(&source.surface, [0, 128, 255]);
        assert!(decoder.next_frame().unwrap().is_none());
        let still =
            LogoOverlay::from_input(&style(), format::input(&path).unwrap(), name, 64, 64).unwrap();
        assert_alpha(&still, [0, 128, 255]);
        let mut background = frame::Video::new(Pixel::YUV420P, 32, 32);
        background.data_mut(0).fill(40);
        background.data_mut(1).fill(128);
        background.data_mut(2).fill(128);
        blend_logo(&mut background, &still, 1.0);
        assert_eq!(background.data(0)[4], 40);
        assert!(background.data(0)[16] > 40);
        assert!(background.data(0)[28] > background.data(0)[16]);
    }
}

#[test]
fn avif_animation_pairs_timed_tracks_instead_of_poster_items_and_rewinds() {
    if !software_av1_available() {
        return;
    }

    let path = fixture("animated-logo-alpha.avif");
    let mut decoder = LogoDecoder::new(format::input(&path).unwrap(), style(), 64, 64).unwrap();

    for _ in 0..3 {
        for index in 0..3 {
            let source = decoder.next_frame().unwrap().unwrap();
            assert_eq!(source.timing.start_us, index * 40_000);
            assert_eq!(source.timing.duration_us, 40_000);
            assert_alpha(
                &source.surface,
                if index == 1 {
                    [255, 128, 0]
                } else {
                    [0, 128, 255]
                },
            );
        }

        assert!(decoder.next_frame().unwrap().is_none());
        decoder.rewind().unwrap();
    }

    decoder.initialize_origin(|| false).unwrap();
    decoder.seek_to(40_000).unwrap();
    let mut reached = false;

    while let Some(source) = decoder.next_frame().unwrap() {
        if source.timing.start_us == 40_000 {
            assert_alpha(&source.surface, [255, 128, 0]);
            reached = true;
        }
    }

    assert!(reached, "seeking must retain matching color/alpha phase");
    decoder.rewind().unwrap();
    assert!(decoder.next_frame_with_cancel(|| true).is_err());
}

#[test]
fn avif_alpha_scaling_keeps_full_range_coverage() {
    if !software_av1_available() {
        return;
    }

    let path = fixture("logo-alpha-10bit.avif");
    let mut decoder = LogoDecoder::new(
        format::input(&path).unwrap(),
        LogoStyle {
            scale: Some("64:64".into()),
            ..style()
        },
        64,
        64,
    )
    .unwrap();
    let source = decoder.next_frame().unwrap().unwrap();
    let alpha = source.surface.frame.data(3);
    assert_eq!(alpha[4], 0);
    assert!(alpha[32].abs_diff(128) <= 1);
    assert_eq!(alpha[60], 255);
}

#[test]
fn avif_premultiplied_stills_and_animation_match_straight_composition() {
    if !software_av1_available() {
        return;
    }

    for (straight, premultiplied, frames) in [
        ("logo-alpha.avif", "logo-premultiplied.avif", 1),
        ("logo-alpha-10bit.avif", "logo-premultiplied-10bit.avif", 1),
        (
            "animated-logo-alpha.avif",
            "animated-logo-premultiplied.avif",
            3,
        ),
    ] {
        for size in [32, 64] {
            let style = LogoStyle {
                scale: Some(format!("{size}:{size}")),
                ..style()
            };
            let mut reference = LogoDecoder::new(
                format::input(&fixture(straight)).unwrap(),
                style.clone(),
                size,
                size,
            )
            .unwrap();
            let mut associated = LogoDecoder::new(
                format::input(&fixture(premultiplied)).unwrap(),
                style,
                size,
                size,
            )
            .unwrap();
            assert_eq!(
                associated.container_alpha,
                Some(crate::surface::Alpha::PremultipliedEncoded)
            );

            for _ in 0..2 {
                for _ in 0..frames {
                    let expected = reference.next_frame().unwrap().unwrap();
                    let actual = associated.next_frame().unwrap().unwrap();
                    assert_eq!(expected.timing, actual.timing);
                    let background = || {
                        let mut frame = frame::Video::new(Pixel::YUV420P, size, size);
                        frame.data_mut(0).fill(60);
                        frame.data_mut(1).fill(90);
                        frame.data_mut(2).fill(160);

                        frame
                    };
                    let mut left = background();
                    let mut right = background();
                    blend_logo(&mut left, &expected.surface, 1.0);
                    blend_logo(&mut right, &actual.surface, 1.0);

                    for plane in 0..3 {
                        for row in 0..left.plane_height(plane) as usize {
                            for x in 0..left.plane_width(plane) as usize {
                                let a = left.data(plane)[row * left.stride(plane) + x];
                                let b = right.data(plane)[row * right.stride(plane) + x];
                                assert!(
                                    a.abs_diff(b) <= 3,
                                    "{premultiplied} size={size} plane={plane} at {x}:{row}: {a} != {b}"
                                );
                            }
                        }
                    }
                }

                assert!(reference.next_frame().unwrap().is_none());
                assert!(associated.next_frame().unwrap().is_none());

                if frames > 1 {
                    reference.rewind().unwrap();
                    associated.rewind().unwrap();
                } else {
                    break;
                }
            }
        }
    }
}
