use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use ffmpeg_next::{
    ChannelLayout,
    format::{Sample, pixel::Pixel, sample::Type},
};

use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        ffmpeg::init().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "ffplayout-hls-recovery-{}-{unique}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        let mut captions = String::from("WEBVTT\n\n");
        for second in 0..30 {
            captions.push_str(&format!(
                "00:00:{second:02}.000 --> 00:00:{second:02}.800\nCaption {second}\n\n"
            ));
        }
        fs::write(directory.join("source.vtt"), captions).unwrap();

        Self(directory)
    }

    fn open(&self) -> EncodedOutput {
        self.try_open("master.m3u8").unwrap()
    }

    fn try_open(&self, name: &str) -> Result<EncodedOutput> {
        self.try_open_packager(name, false)
    }

    fn try_open_packager(&self, name: &str, native: bool) -> Result<EncodedOutput> {
        self.try_open_bitrates(name, native, [96_000; 3])
    }

    fn try_open_bitrates(
        &self,
        name: &str,
        native: bool,
        bitrates: [u64; 3],
    ) -> Result<EncodedOutput> {
        let mut config = OutputConfig::new(320, 180, 25, 48_000);
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        if native {
            config
                .video_options
                .insert("preset".into(), "medium".into());
            config.video_options.insert("tune".into(), "film".into());
            config
                .muxer_options
                .insert("ffplayout_packager".into(), "native".into());
        }

        let mut output = EncodedOutput::open(
            self.0.join(name).to_str().unwrap(),
            &config,
            EncodedFormat::Hls {
                variants: [("high", 320, 180), ("mid", 240, 136), ("low", 160, 90)]
                    .into_iter()
                    .zip(bitrates)
                    .map(|((name, width, height), audio_bitrate)| HlsVariant {
                        name: name.into(),
                        width,
                        height,
                        video_bitrate: 400_000,
                        audio_bitrate,
                    })
                    .collect(),
                subtitle: Some(HlsSubtitle {
                    name: "Captions".into(),
                    language: "en".into(),
                    default: true,
                }),
                segment_seconds: 1,
                list_size: 3,
            },
        )?;
        output.write_vtt_subtitles(
            self.0.join("source.mp4").to_str().unwrap(),
            0,
            0,
            Some(30_000),
        )?;

        Ok(output)
    }

    fn check_references(&self) -> usize {
        let mut count = 0;
        for entry in fs::read_dir(&self.0).unwrap().flatten() {
            if entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "m3u8")
            {
                continue;
            }
            let playlist = fs::read_to_string(entry.path()).unwrap();
            for name in playlist
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
            {
                assert!(
                    self.0.join(name).is_file(),
                    "missing playlist reference: {name}"
                );
                count += 1;
            }
        }

        count
    }

    fn segments(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.0)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "ts" || ext == "vtt")
            })
            .filter(|path| path.file_name().unwrap() != "source.vtt")
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_frames(output: &mut EncodedOutput, frames: std::ops::Range<i64>, audio_pts: &mut i64) {
    for index in frames {
        let mut video = frame::Video::new(Pixel::YUV420P, 320, 180);
        video.set_pts(Some(index));
        video.data_mut(0).fill(64 + (index % 128) as u8);
        video.data_mut(1).fill(128);
        video.data_mut(2).fill(128);
        output.encode_video(&video).unwrap();
        while *audio_pts < (index + 1) * 1920 {
            let size = output.audio_frame_size();
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), size, ChannelLayout::STEREO);
            audio.set_rate(48_000);
            audio.set_pts(Some(*audio_pts));
            for channel in 0..2 {
                for (sample, value) in audio.plane_mut::<f32>(channel).iter_mut().enumerate() {
                    let frequency = if channel == 0 { 440.0 } else { 880.0 };
                    let phase =
                        (*audio_pts + sample as i64) as f32 * frequency * std::f32::consts::TAU
                            / 48_000.0;
                    *value = 0.25 * phase.sin();
                }
            }
            output.encode_audio(&audio).unwrap();
            *audio_pts += size as i64;
        }
        output.advance_vtt_subtitles(index * 40).unwrap();
    }
}

#[test]
fn three_hls_variants_with_vtt_resume_cleanly_through_repeated_restarts() {
    let fixture = Fixture::new();
    let mut previous_sequence = None;
    for restart in 0..4 {
        let mut output = fixture.open();
        write_frames(&mut output, 0..200, &mut 0);
        output.finish().unwrap();
        let references = fixture.check_references();
        assert!(references >= 9);
        let playlist = fs::read_to_string(fixture.0.join("high.m3u8")).unwrap();
        let sequence: u64 = playlist
            .lines()
            .find_map(|line| line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:"))
            .unwrap()
            .parse()
            .unwrap();
        if let Some(previous) = previous_sequence {
            assert!(
                sequence > previous,
                "restart must not reuse segment numbers"
            );
        }
        previous_sequence = Some(sequence);
        eprintln!(
            "clean HLS restart {restart}: sequence={sequence}, existing references={references}"
        );
    }
}

#[test]
fn concurrent_resume_cleanup_is_rejected_without_removing_live_segments() {
    let fixture = Fixture::new();
    let mut output = fixture.open();
    let mut audio_pts = 0;
    write_frames(&mut output, 0..200, &mut audio_pts);
    for iteration in 0..3 {
        let before = fixture.segments();
        // Both an identical output and another master sharing the variants must fail
        // before the existing playlists or retained segments can be modified.
        for name in ["master.m3u8", "alternate.m3u8"] {
            let error = fixture
                .try_open(name)
                .err()
                .expect("overlapping HLS output accepted");

            assert!(error.to_string().contains("already in use"));
        }

        assert!(before.iter().all(|path| path.exists()));
        fixture.check_references();
        let start = 200 + iteration * 100;
        write_frames(&mut output, start..start + 100, &mut audio_pts);
    }
    output.finish().unwrap();
    fixture.check_references();
}

#[test]
fn hls_session_locks_are_released_on_drop_and_failed_acquisition() {
    let fixture = Fixture::new();
    let path = |name: &str| fixture.0.join(name).to_string_lossy().into_owned();
    let second = hls::HlsSession::acquire(&[path("b.m3u8")], None, false).unwrap();

    assert!(hls::HlsSession::acquire(&[path("a.m3u8"), path("b.m3u8")], None, false).is_err());

    let first = hls::HlsSession::acquire(&[path("a.m3u8"), path("./a.m3u8")], None, false).unwrap();

    assert!(hls::HlsSession::acquire(&[path("a.m3u8")], None, false).is_err());
    drop(first);
    drop(second);
    hls::HlsSession::acquire(&[path("a.m3u8"), path("b.m3u8")], None, false).unwrap();
}

#[test]
fn dropping_a_live_hls_muxer_releases_its_paths_for_resume() {
    let fixture = Fixture::new();
    let mut output = fixture.open();

    write_frames(&mut output, 0..200, &mut 0);
    drop(output);

    let mut resumed = fixture.open();

    fixture.check_references();
    write_frames(&mut resumed, 0..100, &mut 0);
    resumed.finish().unwrap();
    fixture.check_references();
}

#[test]
fn fresh_hls_resume_preserves_published_segments_and_unrelated_files() {
    let fixture = Fixture::new();
    let playlist = fixture.0.join("stream.m3u8");
    let contents = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXTINF:1.0,\nstream_7.ts\n";

    fs::write(&playlist, contents).unwrap();
    fs::write(fixture.0.join("stream_7.ts"), b"published").unwrap();
    fs::write(fixture.0.join("stream_6.ts"), b"orphan").unwrap();
    fs::write(fixture.0.join("other_6.ts"), b"unrelated").unwrap();

    let session =
        hls::HlsSession::acquire(&[playlist.to_string_lossy().into_owned()], None, false).unwrap();

    // FFmpeg append_list derives the next segment from this existing sequence.
    assert_eq!(session.prepare_resume_start_number().unwrap(), Some(7));
    assert_eq!(fs::read_to_string(playlist).unwrap(), contents);
    assert_eq!(
        fs::read(fixture.0.join("stream_7.ts")).unwrap(),
        b"published"
    );
    assert!(fixture.0.join("other_6.ts").exists());
    assert!(!fixture.0.join("stream_6.ts").exists());
}

#[test]
fn hls_session_excludes_another_process() {
    const CHILD_PATH: &str = "FFPLAYOUT_TEST_HLS_LOCK_PATH";

    if let Some(path) = std::env::var_os(CHILD_PATH) {
        assert!(
            hls::HlsSession::acquire(&[path.to_string_lossy().into_owned()], None, false).is_err()
        );

        return;
    }

    let fixture = Fixture::new();
    let playlist = fixture.0.join("stream.m3u8");
    let _session =
        hls::HlsSession::acquire(&[playlist.to_string_lossy().into_owned()], None, false).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "output::encoded::recovery_tests::hls_session_excludes_another_process",
        ])
        .env(CHILD_PATH, &playlist)
        .output()
        .unwrap();

    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
}

#[test]
fn standalone_master_named_playlist_resumes_without_deleting_itself() {
    let fixture = Fixture::new();
    let path = fixture.0.join("master.m3u8");
    let config = OutputConfig::new(320, 180, 25, 48_000);

    for _ in 0..2 {
        let mut output = EncodedOutput::open(
            path.to_str().unwrap(),
            &config,
            EncodedFormat::Hls {
                variants: Vec::new(),
                subtitle: None,
                segment_seconds: 1,
                list_size: 3,
            },
        )
        .unwrap();

        write_frames(&mut output, 0..200, &mut 0);
        output.finish().unwrap();
        fixture.check_references();
    }

    let playlist = fs::read_to_string(path).unwrap();
    let sequence: u64 = playlist
        .lines()
        .find_map(|line| line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:"))
        .unwrap()
        .parse()
        .unwrap();

    assert!(sequence > 5, "standalone playlist restarted its numbering");
}

#[test]
fn subtitle_locks_only_reserve_the_configured_first_rendition() {
    let fixture = Fixture::new();
    let path = |name: &str| fixture.0.join(name).to_string_lossy().into_owned();
    let media = [path("stream.m3u8"), path("low.m3u8")];
    let without_subtitles = hls::HlsSession::acquire(&media, None, false).unwrap();
    let independent = hls::HlsSession::acquire(&[path("stream_vtt.m3u8")], None, false).unwrap();

    drop(independent);
    drop(without_subtitles);

    let _with_subtitles = hls::HlsSession::acquire(&media, None, true).unwrap();

    assert!(hls::HlsSession::acquire(&[path("stream_vtt.m3u8")], None, false).is_err());
    hls::HlsSession::acquire(&[path("low_vtt.m3u8")], None, false).unwrap();
}

fn open_ts(path: &std::path::Path) -> format::context::Input {
    // A one-frame tail is valid TS but too small for reliable format probing.
    // HLS clients already know the segment format from the surrounding stream.
    // SAFETY: av_find_input_format returns FFmpeg's static registered descriptor.
    let input_format = unsafe {
        let pointer = ffmpeg::ffi::av_find_input_format(c"mpegts".as_ptr());

        assert!(!pointer.is_null());
        format::format::Input::wrap(pointer.cast_mut())
    };

    format::open(path, &ffmpeg::Format::Input(input_format))
        .unwrap()
        .input()
}

#[test]
fn native_hls_writes_three_real_variants_with_vtt_and_resumes() {
    let fixture = Fixture::new();
    let mut last_number = 0;

    for restart in 0..3 {
        let mut output = fixture.try_open_packager("master.m3u8", true).unwrap();

        write_frames(&mut output, 0..100, &mut 0);
        output.finish().unwrap();
        assert!(fixture.check_references() >= 9);

        let playlist = fs::read_to_string(fixture.0.join("high.m3u8")).unwrap();
        let names: Vec<_> = playlist
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        let last = names.last().unwrap();
        let number = last
            .strip_prefix("high_")
            .unwrap()
            .strip_suffix(".ts")
            .unwrap()
            .parse::<u64>()
            .unwrap();

        assert!(number > last_number);
        last_number = number;

        if restart > 0 {
            assert!(playlist.contains("#EXT-X-DISCONTINUITY"));
        }

        let subtitle = fs::read_to_string(fixture.0.join(format!("high{number}.vtt"))).unwrap();

        assert!(subtitle.contains("X-TIMESTAMP-MAP=LOCAL:"));
        let has_captions = names.iter().any(|name| {
            let number = name
                .strip_prefix("high_")
                .unwrap()
                .strip_suffix(".ts")
                .unwrap();

            fs::read_to_string(fixture.0.join(format!("high{number}.vtt")))
                .unwrap()
                .contains("Caption")
        });

        assert!(has_captions);

        let master = fs::read_to_string(fixture.0.join("master.m3u8")).unwrap();
        let declarations: Vec<_> = master
            .lines()
            .filter(|line| line.starts_with("#EXT-X-STREAM-INF:"))
            .collect();

        assert_eq!(declarations.len(), 3);

        for (index, variant) in ["high", "mid", "low"].into_iter().enumerate() {
            let playlist = fs::read_to_string(fixture.0.join(format!("{variant}.m3u8"))).unwrap();
            let name = playlist
                .lines()
                .find(|line| !line.is_empty() && !line.starts_with('#'))
                .unwrap();
            let mut input = open_ts(&fixture.0.join(name));
            let audio_parameters = input
                .streams()
                .best(ffmpeg::media::Type::Audio)
                .unwrap()
                .parameters();
            let audio_decoder = codec::context::Context::from_parameters(audio_parameters)
                .unwrap()
                .decoder()
                .audio()
                .unwrap();

            assert_eq!(
                audio_decoder.profile(),
                codec::Profile::AAC(codec::profile::AAC::Low)
            );

            let video = input
                .streams()
                .best(ffmpeg::media::Type::Video)
                .unwrap()
                .index();
            let first = input
                .packets()
                .find(|(stream, _)| stream.index() == video)
                .unwrap()
                .1;

            assert!(
                first.is_key(),
                "{variant}: segment must begin with a keyframe"
            );

            // Independently read the RFC 6381 bytes from the emitted TS packet,
            // rather than using the declaration parser to verify its own result.
            let sps = first
                .data()
                .unwrap()
                .windows(7)
                .find(|bytes| bytes[..3] == [0, 0, 1] && bytes[3] & 0x1f == 7)
                .unwrap();
            let expected = format!(
                "CODECS=\"avc1.{:02x}{:02x}{:02x},mp4a.40.2\"",
                sps[4], sps[5], sps[6]
            );

            assert!(declarations[index].contains(&expected), "{master}");
        }
    }
}

#[test]
fn native_hls_vtt_mapping_matches_actual_pes_presentation_time() {
    let fixture = Fixture::new();
    let mut output = fixture.try_open_packager("master.m3u8", true).unwrap();

    write_frames(&mut output, 0..100, &mut 0);
    output.finish().unwrap();

    let playlist = fs::read_to_string(fixture.0.join("high.m3u8")).unwrap();

    for name in playlist
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let number = name
            .strip_prefix("high_")
            .unwrap()
            .strip_suffix(".ts")
            .unwrap();
        let vtt = fs::read_to_string(fixture.0.join(format!("high{number}.vtt"))).unwrap();
        let mapping = vtt
            .lines()
            .find(|line| line.starts_with("X-TIMESTAMP-MAP="))
            .unwrap();
        let mpegts = mapping
            .split("MPEGTS:")
            .nth(1)
            .unwrap()
            .parse::<i64>()
            .unwrap();
        let mut input = open_ts(&fixture.0.join(name));
        let video = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .unwrap()
            .index();
        let (stream, first) = input
            .packets()
            .find(|(stream, _)| stream.index() == video)
            .unwrap();
        let pts = ffmpeg::Rescale::rescale(
            &first.pts().unwrap(),
            stream.time_base(),
            Rational(1, 90_000),
        );

        assert_eq!(pts.rem_euclid(1_i64 << 33), mpegts);
    }
}

#[test]
fn native_and_ffmpeg_packager_switches_clear_owned_media_in_both_directions() {
    let fixture = Fixture::new();

    for native in [false, true, false] {
        let mut output = fixture.try_open_packager("master.m3u8", native).unwrap();

        // Switching backends starts fresh, including retained/orphaned resources.
        assert!(!fixture.0.join("high_999.ts").exists());
        assert!(!fixture.0.join("high999.vtt").exists());
        write_frames(&mut output, 0..100, &mut 0);
        output.finish().unwrap();
        assert!(fixture.check_references() >= 9);

        fs::write(fixture.0.join("high_999.ts"), "old orphan").unwrap();
        fs::write(fixture.0.join("high999.vtt"), "old orphan").unwrap();
        fs::write(fixture.0.join("unrelated_999.ts"), "other output").unwrap();
    }

    assert!(fixture.0.join("unrelated_999.ts").exists());
}

#[test]
fn native_hls_restart_preserves_existing_playlists_and_segments() {
    let fixture = Fixture::new();
    let mut output = fixture.try_open_packager("master.m3u8", true).unwrap();

    write_frames(&mut output, 0..100, &mut 0);
    output.finish().unwrap();

    let playlist = fs::read(fixture.0.join("high.m3u8")).unwrap();
    let segment = fixture
        .segments()
        .into_iter()
        .find(|path| path.extension().is_some_and(|extension| extension == "ts"))
        .unwrap();
    let contents = fs::read(&segment).unwrap();
    let _output = fixture.try_open_packager("master.m3u8", true).unwrap();

    assert_eq!(fs::read(fixture.0.join("high.m3u8")).unwrap(), playlist);
    assert_eq!(fs::read(segment).unwrap(), contents);
}

#[test]
fn native_hls_preserves_all_b_frames_including_the_short_final_segment() {
    let fixture = Fixture::new();
    let mut output = fixture.try_open_packager("master.m3u8", true).unwrap();

    write_frames(&mut output, 0..100, &mut 0);
    output.finish().unwrap();

    let mut timestamps = std::collections::BTreeSet::new();

    for path in fixture.segments().into_iter().filter(|path| {
        path.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("high_")
    }) {
        let mut input = open_ts(&path);
        let video = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .unwrap()
            .index();

        for (stream, packet) in input
            .packets()
            .filter(|(stream, _)| stream.index() == video)
        {
            let pts = ffmpeg::Rescale::rescale(
                &packet.pts().unwrap(),
                stream.time_base(),
                Rational(1, 25),
            );

            assert!(timestamps.insert(pts), "duplicate video frame {pts}");
        }
    }

    assert_eq!(
        timestamps.into_iter().collect::<Vec<_>>(),
        (0..100).collect::<Vec<_>>()
    );
}

#[test]
fn hls_audio_encoders_are_shared_by_bitrate_in_direct_and_threaded_outputs() {
    for native in [false, true] {
        for threaded in [false, true] {
            for bitrates in [[96_000; 3], [96_000, 64_000, 96_000]] {
                let fixture = Fixture::new();
                let mut output = fixture
                    .try_open_bitrates("master.m3u8", native, bitrates)
                    .unwrap();

                assert_eq!(
                    output.audio_streams.len(),
                    if bitrates[0] == bitrates[1] { 1 } else { 2 }
                );
                assert_eq!(
                    output.audio_streams[0].stream_indices,
                    if bitrates[0] == bitrates[1] {
                        vec![1, 3, 5]
                    } else {
                        vec![1, 5]
                    }
                );

                if threaded {
                    output = output.threaded().unwrap();
                }

                let mut samples = 0;
                let frame_size = output.audio_frame_size() as i64;

                write_frames(&mut output, 0..50, &mut samples);
                output.finish().unwrap();

                let audio = |variant: &str| {
                    let playlist =
                        fs::read_to_string(fixture.0.join(format!("{variant}.m3u8"))).unwrap();

                    playlist
                        .lines()
                        .filter(|line| !line.is_empty() && !line.starts_with('#'))
                        .flat_map(|name| {
                            let mut input = open_ts(&fixture.0.join(name));

                            input
                                .packets()
                                .filter(|(stream, _)| {
                                    stream.parameters().medium() == ffmpeg::media::Type::Audio
                                })
                                .map(|(_, packet)| {
                                    (packet.pts(), packet.dts(), packet.data().unwrap().to_vec())
                                })
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                };
                let high = audio("high");

                // AAC emits a priming packet before time zero. Native HLS drops
                // it at the first video boundary; FFmpeg HLS retains it.
                // Count all remaining packets to detect a missing drain tail.
                assert_eq!(high.len() as i64, samples / frame_size + i64::from(!native));
                assert!(
                    high.windows(2)
                        .all(|pair| pair[1].0.unwrap() > pair[0].0.unwrap())
                );
                assert_eq!(high, audio("low"));

                if bitrates[0] == bitrates[1] {
                    assert_eq!(high, audio("mid"));
                } else {
                    assert!(!audio("mid").is_empty());
                }
            }
        }
    }
}
