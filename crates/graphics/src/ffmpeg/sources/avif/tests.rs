use super::*;

fn atom(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut bytes = ((data.len() + 8) as u32).to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(data);

    bytes
}

fn item_metadata(reference: &[u8; 4], target: u16, alpha_type: &[u8]) -> Vec<u8> {
    let mut property = vec![0; 4];
    property.extend(alpha_type);
    let properties = atom(b"ipco", &atom(b"auxC", &property));
    // One item (ID 2), one essential property (index 1).
    let associations = atom(b"ipma", &[0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 1, 0x81]);
    let iprp = atom(b"iprp", &[properties, associations].concat());
    let mut relation = vec![0, 2, 0, 1];
    relation.extend(target.to_be_bytes());
    let iref = atom(b"iref", &[vec![0; 4], atom(reference, &relation)].concat());

    [vec![0; 4], iprp, iref].concat()
}

#[test]
fn only_explicit_alpha_type_and_reference_to_color_establish_an_association() {
    assert_eq!(
        item_alpha(&item_metadata(b"auxl", 1, ALPHA_TYPE), 1).unwrap(),
        Some((2, Alpha::Straight))
    );
    assert_eq!(
        item_alpha(&item_metadata(b"auxl", 3, ALPHA_TYPE), 1).unwrap(),
        None
    );
    assert_eq!(
        item_alpha(&item_metadata(b"thmb", 1, ALPHA_TYPE), 1).unwrap(),
        None
    );
    assert_eq!(
        item_alpha(&item_metadata(b"auxl", 1, b"urn:example:depth\0"), 1).unwrap(),
        None
    );
}

#[test]
fn malformed_sizes_associations_and_truncated_metadata_return_errors() {
    for size in 1_u32..8 {
        let bytes = [size.to_be_bytes().as_slice(), b"auxC"].concat();
        assert!(boxes(&bytes).is_err());
    }

    assert!(boxes(&[0, 0, 0, 20, b'a', b'u', b'x', b'C']).is_err());
    let mut metadata = item_metadata(b"auxl", 1, ALPHA_TYPE);
    let association = metadata
        .windows(4)
        .position(|bytes| bytes == b"ipma")
        .unwrap();
    metadata[association + 15] = 0x82;
    assert!(item_alpha(&metadata, 1).is_err());
    let valid = item_metadata(b"auxl", 1, ALPHA_TYPE);

    for length in 0..valid.len() {
        // Box-boundary prefixes may be valid subsets, but no prefix may panic.
        let _ = item_alpha(&valid[..length], 1);
    }
}

#[test]
fn premultiplied_color_is_reported_to_source_preparation() {
    let mut metadata = item_metadata(b"auxl", 1, ALPHA_TYPE);
    let iref = metadata
        .windows(4)
        .position(|bytes| bytes == b"iref")
        .unwrap();
    let prem = atom(b"prem", &[0, 1, 0, 1, 0, 2]);
    let size = u32::from_be_bytes(metadata[iref - 4..iref].try_into().unwrap()) + prem.len() as u32;
    metadata[iref - 4..iref].copy_from_slice(&size.to_be_bytes());
    metadata.extend(prem);
    assert_eq!(
        item_alpha(&metadata, 1).unwrap(),
        Some((2, Alpha::PremultipliedEncoded))
    );
}

#[test]
fn animated_avif_uses_auxiliary_track_references() {
    let bytes = include_bytes!(
        "../../../../../../tests_assets/storage/media_images/animated-logo-alpha.avif"
    );
    let movie = child(bytes, b"moov").unwrap().unwrap();
    assert_eq!(track_alpha(movie).unwrap(), Some((1, 2, Alpha::Straight)));
    assert!(track_srgb(movie, 1).unwrap());
    assert!(!track_srgb(movie, 2).unwrap());
    let mut unrelated = movie.to_vec();
    let offset = unrelated
        .windows(ALPHA_TYPE.len())
        .position(|bytes| bytes == ALPHA_TYPE)
        .unwrap();
    unrelated[offset + ALPHA_TYPE.len() - 2] = b'x';
    assert_eq!(track_alpha(&unrelated).unwrap(), None);
}

#[test]
fn only_explicit_full_range_srgb_properties_enable_rgb_restoration() {
    let srgb = [b"nclx".as_slice(), &[0, 1, 0, 13, 0, 0, 128]].concat();
    assert!(srgb_color(&srgb).unwrap());

    for (offset, value) in [(5, 9), (7, 16), (9, 1), (10, 0)] {
        let mut other = srgb.clone();
        other[offset] = value;
        assert!(!srgb_color(&other).unwrap());
    }

    for length in 0..srgb.len() {
        assert!(srgb_color(&srgb[..length]).is_err());
    }

    assert!(!srgb_color(b"prof arbitrary ICC profile").unwrap());
    // The colr property belongs to the color item, not its alpha image.
    let ipco = atom(b"ipco", &atom(b"colr", &srgb));
    let ipma = atom(b"ipma", &[0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 1, 1, 0, 2, 0]);
    let metadata = [vec![0; 4], atom(b"iprp", &[ipco, ipma].concat())].concat();
    let properties = item_properties(&metadata).unwrap();
    assert!(properties[&1].srgb);
    assert!(!properties[&2].srgb);
}

#[test]
fn rgb_restoration_keeps_identity_planes_and_preserves_explicit_yuv() {
    for (yuv, rgb) in [
        (Pixel::YUV444P, Pixel::GBRP),
        (Pixel::YUV444P10LE, Pixel::GBRP10LE),
        (Pixel::YUV444P12LE, Pixel::GBRP12LE),
    ] {
        let mut frame = frame::Video::new(yuv, 4, 2);
        frame.set_color_space(Space::Unspecified);

        for plane in 0..3 {
            frame.data_mut(plane).fill(70 + plane as u8 * 20);
        }

        let pixels: Vec<_> = (0..3).map(|plane| frame.data(plane).to_vec()).collect();
        restore_srgb(&mut frame);
        assert_eq!(frame.format(), rgb);
        assert_eq!(frame.color_space(), Space::RGB);
        assert_eq!(frame.color_range(), Range::JPEG);

        for (plane, expected) in pixels.iter().enumerate() {
            assert_eq!(frame.data(plane), expected);
        }
    }

    for (format, space) in [
        (Pixel::YUV444P, Space::BT709),
        (Pixel::YUV420P, Space::Unspecified),
    ] {
        let mut frame = frame::Video::new(format, 4, 2);
        frame.set_color_space(space);
        restore_srgb(&mut frame);
        assert_eq!(frame.format(), format);
        assert_eq!(frame.color_space(), space);
    }
}
