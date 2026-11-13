use ffmpeg_next::{codec, ffi, packet, packet::Mut};

use super::{Codecs, aac, h264, initialization};

const SPS: &[u8] = &[
    0x67, 0x64, 0, 0x1f, 0xac, 0xd9, 0x40, 0x50, 0x05, 0xbb, 0x01, 0x10,
];

fn avcc() -> Vec<u8> {
    let mut data = vec![1, 0x64, 0, 0x1f, 0xff, 0xe1, 0, SPS.len() as u8];

    data.extend_from_slice(SPS);
    data.extend_from_slice(&[1, 0, 2, 0x68, 0x80]);

    data
}

fn asc(fields: &[(u32, usize)]) -> Vec<u8> {
    let mut bits = Vec::new();

    for &(value, count) in fields {
        for bit in (0..count).rev() {
            bits.push(((value >> bit) & 1) as u8);
        }
    }

    bits.chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0, |byte, (index, bit)| byte | bit << (7 - index))
        })
        .collect()
}

#[test]
fn avc_headers_support_avcc_annex_b_profiles_and_constraint_flags() {
    assert_eq!(h264::codec(&avcc()).as_deref(), Some("avc1.64001f"));

    for (profile, flags, level) in [(66, 0xc0, 30), (77, 0x40, 31), (100, 0, 40)] {
        let mut sps = SPS.to_vec();
        sps[1..4].copy_from_slice(&[profile, flags, level]);
        let mut data = vec![0, 0, 0, 1, 0x09, 0xf0, 0, 0, 1];

        data.extend_from_slice(&sps);
        assert_eq!(
            h264::codec(&data),
            Some(format!("avc1.{profile:02x}{flags:02x}{level:02x}"))
        );
    }
}

#[test]
fn invalid_or_conflicting_avc_headers_are_not_advertised() {
    let valid = avcc();

    for end in 0..valid.len() {
        assert!(h264::codec(&valid[..end]).is_none(), "prefix length {end}");
    }

    for (index, value) in [(1, 66), (2, 0xc0), (3, 10), (4, 0xfe), (5, 0xe0), (8, 0x68)] {
        let mut data = valid.clone();
        data[index] = value;

        assert!(h264::codec(&data).is_none(), "invalid byte {index}");
    }

    let mut annex = vec![0, 0, 1];

    annex.extend_from_slice(SPS);
    annex.extend_from_slice(&[0, 0, 1, 0x67, 66, 0xc0, 30, 0x80]);
    assert!(h264::codec(&annex).is_none());
    assert!(h264::codec(&[0, 0, 1, 0x42, 1, 1, 1, 1]).is_none());
}

#[test]
fn aac_lc_explicit_and_implicit_sbr_ps_have_distinct_codec_strings() {
    assert_eq!(aac::codec(&[0x12, 0x10]).as_deref(), Some("mp4a.40.2"));
    assert_eq!(
        aac::codec(&[0x12, 0x10, 0x56, 0xe5, 0]).as_deref(),
        Some("mp4a.40.2")
    );

    for audio_type in [5, 29] {
        let data = asc(&[(audio_type, 5), (7, 4), (1, 4), (4, 4), (2, 5), (0, 3)]);

        assert_eq!(aac::codec(&data), Some(format!("mp4a.40.{audio_type}")));
    }

    for ps in [false, true] {
        let mut fields = vec![
            (2, 5),
            (7, 4),
            (1, 4),
            (0, 3),
            (0x2b7, 11),
            (5, 5),
            (1, 1),
            (4, 4),
        ];

        if ps {
            fields.extend_from_slice(&[(0x548, 11), (1, 1)]);
        }

        assert_eq!(
            aac::codec(&asc(&fields)),
            Some(format!("mp4a.40.{}", if ps { 29 } else { 5 }))
        );
    }
}

#[test]
fn incomplete_reserved_and_unknown_aac_headers_are_not_guessed() {
    for data in [
        &[][..],
        &[0x12],
        &[0xff, 0xff],
        &[0x12, 0x10, 0x56],
        &[0x12, 0x10, 0xff, 0xff],
    ] {
        assert!(aac::codec(data).is_none(), "{data:x?}");
    }

    for fields in [
        vec![(2, 5), (13, 4), (2, 4), (0, 3)],
        vec![(2, 5), (4, 4), (0, 4), (0, 3)],
        vec![(2, 5), (4, 4), (2, 4), (1, 3)],
        vec![(5, 5), (7, 4), (1, 4), (4, 4), (3, 5), (0, 3)],
        vec![(31, 5), (7, 6), (4, 4), (2, 4), (0, 3)],
    ] {
        assert!(aac::codec(&asc(&fields)).is_none());
    }
}

fn parameters(id: codec::Id, data: &[u8]) -> codec::Parameters {
    let mut parameters = codec::Parameters::new();

    // SAFETY: this test owns the parameters and gives FFmpeg an av_mallocz-owned
    // padded buffer; Parameters::drop frees it. The source and destination do not overlap.
    unsafe {
        let raw = &mut *parameters.as_mut_ptr();
        raw.codec_id = id.into();
        raw.extradata =
            ffi::av_mallocz(data.len() + ffi::AV_INPUT_BUFFER_PADDING_SIZE as usize).cast();
        assert!(!raw.extradata.is_null());
        std::ptr::copy_nonoverlapping(data.as_ptr(), raw.extradata, data.len());
        raw.extradata_size = data.len() as i32;
        // Deliberately incorrect metadata: the real header must take precedence.
        raw.profile = 66;
        raw.level = 10;
    }

    parameters
}

#[test]
fn real_initialization_overrides_wrong_profile_hints_but_not_wrong_codec_ids() {
    let video = parameters(codec::Id::H264, &avcc());
    let audio = parameters(codec::Id::AAC, &[0x12, 0x10]);

    assert_eq!(
        Codecs::new(&video, &audio).value().as_deref(),
        Some("avc1.64001f,mp4a.40.2")
    );
    assert!(
        Codecs::new(&parameters(codec::Id::HEVC, &avcc()), &audio)
            .value()
            .is_none()
    );
    assert!(
        Codecs::new(&video, &parameters(codec::Id::OPUS, &[0x12, 0x10]))
            .value()
            .is_none()
    );
    assert!(
        Codecs::new(&video, &parameters(codec::Id::AAC, &[0xff]))
            .value()
            .is_none()
    );
}

#[test]
fn missing_sps_is_discovered_in_the_first_key_packet() {
    let mut codecs = Codecs::new(
        &parameters(codec::Id::H264, &[]),
        &parameters(codec::Id::AAC, &[0x12, 0x10]),
    );
    let mut data = vec![0, 0, 0, 1];

    data.extend_from_slice(SPS);

    let mut packet = ffmpeg_next::Packet::copy(&data);

    assert!(codecs.value().is_none());
    assert!(codecs.observe(&packet).is_none());
    packet.set_flags(packet::Flags::KEY);
    assert_eq!(
        codecs.observe(&packet).as_deref(),
        Some("avc1.64001f,mp4a.40.2")
    );
    assert!(codecs.observe(&packet).is_none());
}

#[test]
fn late_audio_initialization_is_required_instead_of_guessing_from_adts() {
    let mut codecs = Codecs::new(
        &parameters(codec::Id::H264, &avcc()),
        &parameters(codec::Id::AAC, &[]),
    );
    let mut packet = ffmpeg_next::Packet::copy(&[0xff, 0xf1, 0x50, 0x80, 0, 0x1f, 0xfc]);

    packet.set_stream(1);
    assert!(codecs.observe(&packet).is_none());

    // SAFETY: the test exclusively owns the packet; FFmpeg allocates and owns
    // the side-data buffer, which has exactly the two bytes copied below.
    unsafe {
        let data = ffi::av_packet_new_side_data(
            packet.as_mut_ptr(),
            ffi::AVPacketSideDataType::AV_PKT_DATA_NEW_EXTRADATA,
            2,
        );

        assert!(!data.is_null());
        std::ptr::copy_nonoverlapping([0x12, 0x10].as_ptr(), data, 2);
    }

    assert_eq!(
        codecs.observe(&packet).as_deref(),
        Some("avc1.64001f,mp4a.40.2")
    );
}

#[test]
fn actual_aac_encoders_report_lc_he_and_hev2_initialization() {
    ffmpeg_next::init().unwrap();

    for (name, profiles) in [
        ("aac", &[(codec::profile::AAC::Low, 2)][..]),
        (
            "libfdk_aac",
            &[
                (codec::profile::AAC::Low, 2),
                (codec::profile::AAC::HE, 5),
                (codec::profile::AAC::HEv2, 29),
            ][..],
        ),
    ] {
        let Some(encoder) = codec::encoder::find_by_name(name) else {
            eprintln!("optional encoder {name} unavailable");
            continue;
        };

        for &(profile, object_type) in profiles {
            let mut context = crate::utils::config::audio_encoder_context(
                encoder,
                &Default::default(),
                48_000,
                32_000,
                ffmpeg_next::Rational(1, 48_000),
                true,
            )
            .unwrap();

            // SAFETY: the unopened context is exclusively owned by this test.
            unsafe {
                (*context.as_mut_ptr()).profile = codec::Profile::AAC(profile).into();
            }

            let encoder = context.open_as(encoder).unwrap();
            let parameters = codec::Parameters::from(&encoder);

            assert_eq!(
                aac::codec(initialization(&parameters)),
                Some(format!("mp4a.40.{object_type}")),
                "{name} {profile:?}: {:x?}",
                initialization(&parameters)
            );
        }
    }
}
