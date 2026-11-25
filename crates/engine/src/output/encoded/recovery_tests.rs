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
        self.try_open_container(name, native, bitrates, None)
    }

    fn try_open_container(
        &self,
        name: &str,
        native: bool,
        bitrates: [u64; 3],
        container: Option<&str>,
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

        if let Some(container) = container {
            config
                .muxer_options
                .insert("hls_segment_type".into(), container.into());
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
    assert!(!session.requires_replacement(None).unwrap());
    assert_eq!(session.resume_start_number().unwrap(), Some(7));
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
fn native_fmp4_keeps_video_audio_and_webvtt_on_the_program_clock() {
    for (encoder, declaration) in [
        ("libx264", "avc1."),
        ("libx265", "hvc1."),
        ("libsvtav1", "av01."),
    ] {
        ffmpeg::init().unwrap();
        if codec::encoder::find_by_name(encoder).is_none() {
            continue;
        }

        let fixture = Fixture::new();
        let mut config = OutputConfig::new(320, 180, 25, 48_000);
        config.video_codec = encoder.into();
        config.video_options.insert(
            "preset".into(),
            if encoder == "libsvtav1" {
                "10"
            } else if encoder == "libx264" {
                "medium"
            } else {
                "ultrafast"
            }
            .into(),
        );
        config
            .muxer_options
            .insert("ffplayout_packager".into(), "native".into());
        if encoder == "libx264" {
            config
                .muxer_options
                .insert("hls_segment_type".into(), "fmp4".into());
        }

        let first = if encoder == "libx264" { 0 } else { 53 };
        for start in [first, 125] {
            let mut output = EncodedOutput::open(
                fixture.0.join("high.m3u8").to_str().unwrap(),
                &config,
                EncodedFormat::Hls {
                    variants: [("high", 320, 180), ("low", 160, 96)]
                        .into_iter()
                        .map(|(name, width, height)| HlsVariant {
                            name: name.into(),
                            width,
                            height,
                            video_bitrate: 400_000,
                            audio_bitrate: 96_000,
                        })
                        .collect(),
                    subtitle: Some(HlsSubtitle {
                        name: "Captions".into(),
                        language: "en".into(),
                        default: true,
                    }),
                    segment_seconds: 1,
                    list_size: 9,
                },
            )
            .unwrap();
            output
                .write_vtt_subtitles(
                    fixture.0.join("source.mp4").to_str().unwrap(),
                    0,
                    0,
                    Some(30_000),
                )
                .unwrap();
            write_frames(&mut output, start..start + 68, &mut (start * 1920));
            output.finish().unwrap();
        }

        let media = fs::read_to_string(fixture.0.join("high.m3u8")).unwrap();
        let low = fs::read_to_string(fixture.0.join("low.m3u8")).unwrap();
        let subtitle = fs::read_to_string(fixture.0.join("high_vtt.m3u8")).unwrap();
        let master = fs::read_to_string(fixture.0.join("master.m3u8")).unwrap();
        assert!(
            master.contains(&format!("CODECS=\"{declaration}")),
            "{encoder}: {master}"
        );
        assert!(media.contains("#EXT-X-VERSION:7"));
        assert_eq!(
            media.matches("#EXT-X-MAP:").count(),
            2,
            "{encoder}: {media}"
        );
        assert!(media.contains("\n#EXT-X-DISCONTINUITY\n"));
        assert_eq!(
            media.matches("#EXTINF:").count(),
            subtitle.matches("#EXTINF:").count()
        );
        assert_eq!(
            media
                .lines()
                .filter(|line| line.starts_with("#EXTINF:"))
                .collect::<Vec<_>>(),
            low.lines()
                .filter(|line| line.starts_with("#EXTINF:"))
                .collect::<Vec<_>>()
        );
        assert!(low.contains("#EXT-X-MAP:URI=\"low_"));

        let mut init = "";
        let mut starts_us = Vec::new();
        let mut total_video = 0;
        for line in media.lines() {
            if let Some(uri) = line
                .strip_prefix("#EXT-X-MAP:URI=\"")
                .and_then(|uri| uri.strip_suffix('"'))
            {
                init = uri;
            } else if line.ends_with(".m4s") {
                let mut bytes = fs::read(fixture.0.join(init)).unwrap();
                bytes.extend(fs::read(fixture.0.join(line)).unwrap());
                let input_file = fixture.0.join("probe.mp4");
                fs::write(&input_file, bytes).unwrap();
                let mut input = format::input(&input_file).unwrap();
                let stream = input.streams().best(ffmpeg::media::Type::Video).unwrap();
                let video = stream.index();
                let audio = input
                    .streams()
                    .best(ffmpeg::media::Type::Audio)
                    .unwrap()
                    .index();
                let mut decoder = codec::context::Context::from_parameters(stream.parameters())
                    .unwrap()
                    .decoder()
                    .video()
                    .unwrap();
                let mut first_audio = None;
                let mut first_video = None;
                let mut decoded = 0;
                let mut raw = frame::Video::empty();

                for (stream, packet) in input.packets() {
                    use ffmpeg::Rescale;
                    let pts = packet
                        .pts()
                        .unwrap()
                        .rescale(stream.time_base(), ffmpeg::rescale::TIME_BASE);
                    if stream.index() == video {
                        if first_video.is_none() {
                            assert!(packet.is_key(), "{encoder}: {line}");
                            first_video = Some(pts);
                        }
                        decoder.send_packet(&packet).unwrap();
                        while decoder.receive_frame(&mut raw).is_ok() {
                            decoded += 1;
                        }
                    } else if stream.index() == audio {
                        first_audio.get_or_insert(pts);
                    }
                }
                decoder.send_eof().unwrap();
                while decoder.receive_frame(&mut raw).is_ok() {
                    decoded += 1;
                }
                assert!(decoded > 0, "{encoder}: {line}");
                total_video += decoded;
                let first_video = first_video.unwrap();
                assert!(
                    first_video.abs_diff(first_audio.unwrap()) <= 22_000,
                    "{encoder}: {line} audio={first_audio:?} video={first_video}"
                );
                starts_us.push(first_video);
            }
        }

        assert!(
            starts_us.windows(2).all(|pair| pair[0] < pair[1]),
            "{encoder}: {starts_us:?}"
        );
        assert_eq!(total_video, 136, "{encoder}");
        // Exercise the actual HLS demuxer as well as isolated fragments.
        let playlist = fixture.0.join("low.m3u8");
        fs::write(&playlist, format!("{low}#EXT-X-ENDLIST\n")).unwrap();
        let mut input = format::input(&playlist).unwrap();
        assert_eq!(
            input
                .packets()
                .filter(|(stream, _)| stream.parameters().medium() == ffmpeg::media::Type::Video)
                .count(),
            136,
            "{encoder}: media playlist lost video packets"
        );
        if first == 0 {
            assert!(
                (0..=120_000).contains(&starts_us[0]),
                "{encoder}: {starts_us:?}"
            );
        } else {
            assert!(
                (2_120_000..=2_122_000).contains(&starts_us[0]),
                "{encoder}: {starts_us:?}"
            );
        }
        for (line, pts) in subtitle
            .lines()
            .filter(|uri| uri.ends_with(".vtt"))
            .zip(starts_us)
        {
            let vtt = fs::read_to_string(fixture.0.join(line)).unwrap();
            let map: i64 = vtt
                .lines()
                .find_map(|line| line.split_once("MPEGTS:"))
                .unwrap()
                .1
                .parse()
                .unwrap();
            assert!(
                map.abs_diff(pts * 90 / 1000) <= 90,
                "{encoder}: {line} map={map} pts={pts}"
            );
        }
    }
}

#[test]
fn native_fmp4_replaces_incompatible_initialization_after_the_first_segment() {
    let fixture = Fixture::new();
    let path = fixture.0.join("high.m3u8");
    let mut config = OutputConfig::new(320, 180, 25, 48_000);
    config
        .video_options
        .insert("preset".into(), "ultrafast".into());
    config
        .muxer_options
        .insert("ffplayout_packager".into(), "native".into());
    config
        .muxer_options
        .insert("hls_segment_type".into(), "fmp4".into());
    let open = |width, height| {
        EncodedOutput::open(
            path.to_str().unwrap(),
            &config,
            EncodedFormat::Hls {
                variants: vec![HlsVariant {
                    name: "high".into(),
                    width,
                    height,
                    video_bitrate: 400_000,
                    audio_bitrate: 96_000,
                }],
                subtitle: None,
                segment_seconds: 1,
                list_size: 9,
            },
        )
        .unwrap()
    };

    let mut output = open(320, 180);
    write_frames(&mut output, 0..50, &mut 0);
    output.finish().unwrap();
    let previous = fs::read_to_string(&path).unwrap();
    assert_eq!(previous.matches("#EXTINF:").count(), 2);
    assert_eq!(previous.matches("#EXT-X-MAP:").count(), 1);

    drop(open(160, 96));
    assert_eq!(fs::read_to_string(&path).unwrap(), previous);
    let mut output = open(160, 96);
    assert_eq!(fs::read_to_string(&path).unwrap(), previous);
    write_frames(&mut output, 50..100, &mut (50 * 1920));
    output.finish().unwrap();

    let current = fs::read_to_string(&path).unwrap();
    assert_eq!(current.matches("#EXTINF:").count(), 2);
    assert_eq!(current.matches("#EXT-X-MAP:").count(), 1);
    assert!(current.contains("#EXT-X-MEDIA-SEQUENCE:2"));
    for line in previous.lines().filter(|line| line.ends_with(".m4s")) {
        assert!(!current.contains(line));
        assert!(fixture.0.join(line).is_file());
    }
    let old_init = previous
        .lines()
        .find_map(|line| line.strip_prefix("#EXT-X-MAP:URI=\"")?.strip_suffix('"'))
        .unwrap();
    assert!(fixture.0.join(old_init).is_file());

    fs::write(&path, format!("{current}#EXT-X-ENDLIST\n")).unwrap();
    let mut input = format::input(&path).unwrap();
    let stream = input.streams().best(ffmpeg::media::Type::Video).unwrap();
    let video = stream.index();
    let mut decoder = codec::context::Context::from_parameters(stream.parameters())
        .unwrap()
        .decoder()
        .video()
        .unwrap();
    let mut decoded = Vec::new();
    let mut raw = frame::Video::empty();
    for (_, packet) in input
        .packets()
        .filter(|(stream, _)| stream.index() == video)
    {
        decoder.send_packet(&packet).unwrap();
        while decoder.receive_frame(&mut raw).is_ok() {
            decoded.push((raw.width(), raw.height()));
        }
    }
    decoder.send_eof().unwrap();
    while decoder.receive_frame(&mut raw).is_ok() {
        decoded.push((raw.width(), raw.height()));
    }
    assert_eq!(decoded, vec![(160, 96); 50]);
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
fn native_and_ffmpeg_switches_keep_published_media_until_replacement_is_ready() {
    let fixture = Fixture::new();
    let playlists = [
        "master.m3u8",
        "high.m3u8",
        "mid.m3u8",
        "low.m3u8",
        "high_vtt.m3u8",
    ];
    let read = || playlists.map(|name| fs::read(fixture.0.join(name)).unwrap());
    let media_range = |playlist: &[u8]| {
        let playlist = std::str::from_utf8(playlist).unwrap();
        let sequence: u64 = playlist
            .lines()
            .find_map(|line| line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:"))
            .unwrap()
            .parse()
            .unwrap();
        let count = playlist.matches("#EXTINF:").count() as u64;

        sequence..sequence + count
    };
    let mut output = fixture.try_open_packager("master.m3u8", false).unwrap();
    write_frames(&mut output, 0..100, &mut 0);
    output.finish().unwrap();

    // The old generation has already crossed several timestamp resets.
    for name in &playlists[1..] {
        let path = fixture.0.join(name);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(
            path,
            text.replacen("#EXTM3U\n", "#EXTM3U\n#EXT-X-DISCONTINUITY-SEQUENCE:7\n", 1),
        )
        .unwrap();
    }
    let generation = |playlist: &[u8]| {
        let text = std::str::from_utf8(playlist).unwrap();
        let base: u64 = text
            .lines()
            .find_map(|line| line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:"))
            .unwrap()
            .parse()
            .unwrap();
        base + text
            .lines()
            .filter(|line| *line == "#EXT-X-DISCONTINUITY")
            .count() as u64
    };
    for (native, container) in [(true, Some("mpegts")), (true, Some("fmp4")), (false, None)] {
        let open = || {
            fixture
                .try_open_container("master.m3u8", native, [96_000; 3], container)
                .unwrap()
        };
        let previous = read();
        let old_high = std::str::from_utf8(&previous[1]).unwrap();
        let old_segment = fixture.0.join(
            old_high
                .lines()
                .find(|line| !line.is_empty() && !line.starts_with('#'))
                .unwrap(),
        );
        let old_init = old_high
            .lines()
            .find_map(|line| line.strip_prefix("#EXT-X-MAP:URI=\"")?.strip_suffix('"'))
            .map(|uri| (fixture.0.join(uri), fs::read(fixture.0.join(uri)).unwrap()));
        let old_media = fs::read(&old_segment).unwrap();
        fs::write(fixture.0.join("high_999.ts"), "old orphan").unwrap();
        fs::write(fixture.0.join("high999.vtt"), "old orphan").unwrap();
        fs::write(fixture.0.join("unrelated_999.ts"), "other output").unwrap();

        drop(open());
        assert_eq!(read(), previous);
        assert_eq!(fs::read(&old_segment).unwrap(), old_media);
        let mut output = open();
        assert_eq!(read(), previous);
        write_frames(&mut output, 0..100, &mut 0);
        output.finish().unwrap();
        assert!(fixture.check_references() >= 9);
        let current = read();
        assert_ne!(current, previous);
        let expected = generation(&previous[1]) + 1;
        assert!(
            current[1..]
                .iter()
                .all(|playlist| generation(playlist) == expected)
        );
        assert!(media_range(&current[1]).start >= media_range(&previous[1]).end);
        assert_eq!(fs::read(old_segment).unwrap(), old_media);
        if let Some((path, bytes)) = old_init {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        assert!(!fixture.0.join("high_999.ts").exists());
        assert!(!fixture.0.join("high999.vtt").exists());
    }

    assert!(fixture.0.join("unrelated_999.ts").exists());
}

#[test]
fn ffmpeg_restart_after_native_switch_preserves_segment_generations() {
    let fixture = Fixture::new();
    let read = || {
        ["high", "mid", "low", "high_vtt"].map(|name| {
            let text = fs::read_to_string(fixture.0.join(format!("{name}.m3u8"))).unwrap();
            let mut generation = 0_u64;
            let mut sequence = 0_u64;
            let mut segments = Vec::new();
            for line in text.lines() {
                if let Some(base) = line.strip_prefix("#EXT-X-DISCONTINUITY-SEQUENCE:") {
                    generation = base.parse().unwrap();
                } else if let Some(first) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
                    sequence = first.parse().unwrap();
                } else if line == "#EXT-X-DISCONTINUITY" {
                    generation += 1;
                } else if !line.is_empty() && !line.starts_with('#') {
                    segments.push((sequence, line.to_owned(), generation));
                    sequence += 1;
                }
            }
            segments
        })
    };
    let run = |native, frames| {
        let mut output = fixture.try_open_packager("master.m3u8", native).unwrap();
        write_frames(&mut output, 0..frames, &mut 0);
        output.finish().unwrap();
    };
    run(true, 75);
    for name in ["high", "mid", "low", "high_vtt"] {
        let path = fixture.0.join(format!("{name}.m3u8"));
        let text = fs::read_to_string(&path).unwrap();
        fs::write(
            path,
            text.replace("DISCONTINUITY-SEQUENCE:0", "DISCONTINUITY-SEQUENCE:7"),
        )
        .unwrap();
    }
    run(false, 100);
    let mut previous = read();
    for _ in 0..3 {
        run(false, 25);
        let current = read();
        for (old, new) in previous.iter().zip(&current) {
            let last = old.last().unwrap();
            assert!(last.2 >= 8);
            assert_eq!(
                &new[..new.len() - 1],
                &old[1..],
                "retained segment identities"
            );
            assert_eq!(new.last().unwrap().0, last.0 + 1);
            assert_eq!(new.last().unwrap().2, last.2 + 1);
            for (_, uri, _) in new {
                assert!(fixture.0.join(uri).is_file(), "{uri}");
            }
        }
        previous = current;
    }
    run(false, 150);
    for (old, new) in previous.iter().zip(read()) {
        let last = old.last().unwrap();
        assert!(new.first().unwrap().0 > last.0 + 1);
        assert!(
            new.iter()
                .all(|(_, uri, generation)| *generation == last.2 + 1
                    && fixture.0.join(uri).is_file())
        );
    }
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

#[test]
fn native_hls_real_encoders_join_six_second_grid_at_arbitrary_pts_and_frame_rates() {
    for fps in [25, 30, 50] {
        for start in [1, i64::from(fps) * 2 - 1, i64::from(fps) * 6 - 1] {
            let fixture = Fixture::new();
            let mut config = OutputConfig::new(64, 48, fps, 48_000);
            config
                .video_options
                .insert("preset".into(), "medium".into());
            config.video_options.insert("tune".into(), "film".into());
            config
                .muxer_options
                .insert("ffplayout_packager".into(), "native".into());
            let variants = [("high", 64, 48), ("low", 32, 24)]
                .into_iter()
                .map(|(name, width, height)| HlsVariant {
                    name: name.into(),
                    width,
                    height,
                    video_bitrate: 200_000,
                    audio_bitrate: 96_000,
                })
                .collect();
            let mut output = EncodedOutput::open(
                fixture.0.join("master.m3u8").to_str().unwrap(),
                &config,
                EncodedFormat::Hls {
                    variants,
                    subtitle: Some(HlsSubtitle {
                        name: "Captions".into(),
                        language: "en".into(),
                        default: false,
                    }),
                    segment_seconds: 6,
                    list_size: 10,
                },
            )
            .unwrap();
            let mut audio_pts = start * 48_000 / i64::from(fps);
            let frame_count = i64::from(fps) * 13;

            output
                .write_vtt_subtitles(
                    fixture.0.join("source.mp4").to_str().unwrap(),
                    0,
                    0,
                    Some(30_000),
                )
                .unwrap();

            for pts in start..start + frame_count {
                let mut video = frame::Video::new(Pixel::YUV420P, 64, 48);
                video.set_pts(Some(pts));
                video.data_mut(0).fill(if pts % 37 < 10 { 16 } else { 235 });
                video.data_mut(1).fill(128);
                video.data_mut(2).fill(128);
                output.encode_video(&video).unwrap();

                while audio_pts < (pts + 1) * 48_000 / i64::from(fps) {
                    let size = output.audio_frame_size();
                    let mut audio =
                        frame::Audio::new(Sample::F32(Type::Planar), size, ChannelLayout::STEREO);
                    audio.set_rate(48_000);
                    audio.set_pts(Some(audio_pts));
                    audio.plane_mut::<f32>(0).fill(0.1);
                    audio.plane_mut::<f32>(1).fill(-0.1);
                    output.encode_audio(&audio).unwrap();
                    audio_pts += size as i64;
                }
            }

            output.finish().unwrap();

            let mut reference = None;

            for name in ["high", "low", "high_vtt"] {
                let text = fs::read_to_string(fixture.0.join(format!("{name}.m3u8"))).unwrap();
                let durations: Vec<f64> = text
                    .lines()
                    .filter_map(|line| line.strip_prefix("#EXTINF:"))
                    .map(|value| value.trim_end_matches(',').parse().unwrap())
                    .collect();

                assert!(text.contains("#EXT-X-TARGETDURATION:6\n"));
                assert!(
                    (durations[0] - (6.0 - start as f64 / f64::from(fps))).abs() < 0.000002,
                    "fps={fps}, start={start}: {durations:?}"
                );
                assert!(
                    durations[1..durations.len() - 1]
                        .iter()
                        .all(|duration| (*duration - 6.0).abs() < 0.000002)
                );
                assert!(durations.iter().all(|duration| duration.round() <= 6.0));

                if let Some(reference) = &reference {
                    assert_eq!(&durations, reference);
                } else {
                    reference = Some(durations);
                }

                if name == "high_vtt" {
                    continue;
                }

                let mut decoded = 0;

                for uri in text
                    .lines()
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                {
                    let mut input = open_ts(&fixture.0.join(uri));
                    let stream = input.streams().best(ffmpeg::media::Type::Video).unwrap();
                    let index = stream.index();
                    let mut decoder = codec::context::Context::from_parameters(stream.parameters())
                        .unwrap()
                        .decoder()
                        .video()
                        .unwrap();
                    let mut first = true;
                    let mut drain = |decoder: &mut ffmpeg::decoder::Video| {
                        let mut frame = frame::Video::empty();

                        while decoder.receive_frame(&mut frame).is_ok() {
                            decoded += 1;
                        }
                    };

                    for (stream, packet) in input.packets() {
                        if stream.index() != index {
                            continue;
                        }

                        if first {
                            assert!(packet.is_key(), "segment {uri} must decode independently");
                            first = false;
                        }

                        decoder.send_packet(&packet).unwrap();
                        drain(&mut decoder);
                    }

                    decoder.send_eof().unwrap();
                    drain(&mut decoder);
                    assert!(!first);
                }

                assert_eq!(
                    decoded, frame_count,
                    "lost frames: fps={fps}, start={start}, variant={name}"
                );
            }
        }
    }
}
