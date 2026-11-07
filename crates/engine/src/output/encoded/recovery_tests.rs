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
        let mut config = OutputConfig::new(320, 180, 25, 48_000);
        config
            .video_options
            .insert("preset".into(), "ultrafast".into());
        let mut output = EncodedOutput::open(
            self.0.join(name).to_str().unwrap(),
            &config,
            EncodedFormat::Hls {
                variants: [("high", 320, 180), ("mid", 240, 136), ("low", 160, 90)]
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
                audio.plane_mut::<f32>(channel).fill(0.0);
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
