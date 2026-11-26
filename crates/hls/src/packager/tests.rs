use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, bail};
use jiff::{SignedDuration, Timestamp};

use super::*;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("ff-hls-test-{}-{id}", std::process::id()));

        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();

        Self(path)
    }

    fn config(&self, count: usize, list_size: u32) -> Config {
        Config {
            directory: self.0.clone(),
            renditions: (0..count)
                .map(|index| RenditionConfig {
                    name: format!("v{index}"),
                    width: 320,
                    height: 180,
                    bandwidth: 500_000,
                    codecs: String::new(),
                })
                .collect(),
            subtitle: Some(Subtitle {
                name: "Captions".into(),
                language: "en".into(),
                default: false,
            }),
            master_playlist: Some("master.m3u8".into()),
            segment_seconds: 1,
            list_size,
            delete_threshold: 1,
            program_date_time: false,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Writer {
    fail_rendition: Option<usize>,
}

impl SegmentWriter<()> for Writer {
    fn write_segment(
        &mut self,
        path: &Path,
        packets: &[MediaPacket<()>],
        rendition: usize,
    ) -> Result<()> {
        if self.fail_rendition == Some(rendition) {
            bail!("simulated storage failure");
        }

        let video: Vec<_> = packets
            .iter()
            .filter(|packet| packet.stream == rendition * 2)
            .collect();
        let first = video.first().unwrap();

        assert!(first.keyframe, "segment must begin on a video keyframe");
        assert!(!path.exists(), "segment filename must never be reused");
        fs::write(path, format!("{}:{}", first.pts_us, video.len()))?;

        Ok(())
    }
}

#[derive(Default)]
struct FragmentWriter {
    initialization: Option<String>,
}

impl SegmentWriter<()> for FragmentWriter {
    fn extension(&self) -> &'static str {
        "m4s"
    }

    fn initialization(&self, _rendition: usize) -> Option<&str> {
        self.initialization.as_deref()
    }

    fn write_segment(
        &mut self,
        path: &Path,
        _packets: &[MediaPacket<()>],
        _rendition: usize,
    ) -> Result<()> {
        if self.initialization.is_none() {
            let init = path.with_extension("init.mp4");
            fs::write(&init, b"init")?;
            self.initialization = Some(init.file_name().unwrap().to_string_lossy().into_owned());
        }
        fs::write(path, b"fragment")?;

        Ok(())
    }
}

#[test]
fn fmp4_initialization_survives_resume_and_retires_with_its_last_fragment() {
    let fixture = Fixture::new();
    {
        let mut packager = Packager::new(fixture.config(1, 3), FragmentWriter::default()).unwrap();
        feed(&mut packager, 1, 0, 100).unwrap();
        packager.finish().unwrap();
    }
    let first_init = fixture.0.join("v0_0.init.mp4");
    let playlist = fixture.0.join("v0.m3u8");
    assert!(first_init.exists());

    let mut packager = Packager::new(fixture.config(1, 3), FragmentWriter::default()).unwrap();
    feed(&mut packager, 1, 4_000_000, 51).unwrap();
    let mixed = fs::read_to_string(&playlist).unwrap();
    assert_eq!(mixed.matches("#EXT-X-MAP:").count(), 2);
    assert!(first_init.exists());
    feed(&mut packager, 1, 6_040_000, 100).unwrap();
    packager.finish().unwrap();

    let retired = fs::read_to_string(playlist).unwrap();
    assert_eq!(retired.matches("#EXT-X-MAP:").count(), 1);
    assert!(!first_init.exists());
    assert!(fixture.0.join("v0_4.init.mp4").exists());
}

fn packet(stream: usize, pts_us: i64, keyframe: bool) -> MediaPacket<()> {
    MediaPacket {
        stream,
        pts_us,
        dts_us: pts_us,
        duration_us: 40_000,
        keyframe,
        bytes: 100,
        payload: (),
    }
}

fn feed<W: SegmentWriter<()>>(
    packager: &mut Packager<(), W>,
    renditions: usize,
    base: i64,
    frames: i64,
) -> Result<()> {
    feed_with_clock(packager, renditions, base, frames, Timestamp::now)
}

fn feed_with_clock<W: SegmentWriter<()>>(
    packager: &mut Packager<(), W>,
    renditions: usize,
    base: i64,
    frames: i64,
    clock: impl Fn() -> Timestamp,
) -> Result<()> {
    for frame in 0..frames {
        for rendition in 0..renditions {
            let pts = base + frame * 40_000;

            packager.write_with_clock(packet(rendition * 2, pts, frame % 25 == 0), &clock)?;
            packager.write_with_clock(packet(rendition * 2 + 1, pts, false), &clock)?;
        }
    }

    Ok(())
}

#[test]
fn publishes_aligned_variants_and_cross_segment_cues_only_after_media() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(3, 3),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    packager
        .write_cue(Cue {
            start_ms: 500,
            end_ms: 2500,
            text: "Crossing cue".into(),
        })
        .unwrap();
    feed(&mut packager, 3, 0, 24).unwrap();
    assert!(!fixture.0.join("master.m3u8").exists());
    packager.write(packet(0, 1_000_000, true)).unwrap();
    assert!(!fixture.0.join("master.m3u8").exists());

    // Finish supplying the first rendition and the matching keyframes of the others.
    packager.write(packet(1, 1_000_000, false)).unwrap();

    for stream in 2..6 {
        packager
            .write(packet(stream, 1_000_000, stream.is_multiple_of(2)))
            .unwrap();
    }

    assert!(fixture.0.join("master.m3u8").exists());

    for index in 0..3 {
        let playlist = fs::read_to_string(fixture.0.join(format!("v{index}.m3u8"))).unwrap();

        assert!(playlist.contains("#EXTINF:1.000000,"));
        assert!(fixture.0.join(format!("v{index}_0.ts")).exists());
    }

    feed(&mut packager, 3, 1_040_000, 25).unwrap();
    packager.finish().unwrap();

    for number in 0..=1 {
        let subtitle = fs::read_to_string(fixture.0.join(format!("v0{number}.vtt"))).unwrap();

        assert!(subtitle.contains("Crossing cue"));
        assert!(subtitle.contains("00:00:00.500 --> 00:00:02.500"));
    }

    let second = fs::read_to_string(fixture.0.join("v01.vtt")).unwrap();

    assert!(second.contains("LOCAL:00:00:01.000,MPEGTS:90000"));
}

#[test]
fn date_time_mapping_is_shared_across_renditions_and_preserved_on_resume() {
    for enabled in [false, true] {
        let fixture = Fixture::new();
        let mut previous_dates = Vec::new();

        for session in 0..2 {
            let mut config = fixture.config(2, 100);
            config.program_date_time = enabled;

            let mut packager = Packager::new(
                config,
                Writer {
                    fail_rendition: None,
                },
            )
            .unwrap();

            let first = Timestamp::UNIX_EPOCH
                .checked_add(SignedDuration::from_secs(session as i64 * 4))
                .unwrap();

            feed_with_clock(&mut packager, 2, 0, 100, || first).unwrap();
            packager.finish().unwrap();
            drop(packager);

            let playlists = ["v0.m3u8", "v1.m3u8", "v0_vtt.m3u8"].map(|name| {
                let text = fs::read_to_string(fixture.0.join(name)).unwrap();

                text.lines()
                    .filter_map(|line| line.strip_prefix("#EXT-X-PROGRAM-DATE-TIME:"))
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            });

            assert_eq!(playlists[0], playlists[1]);
            assert_eq!(playlists[0], playlists[2]);
            assert_eq!(
                playlists[0].len(),
                if enabled { (session + 1) * 4 } else { 0 }
            );
            assert!(playlists[0].starts_with(&previous_dates));

            let dates = &playlists[0][previous_dates.len()..];

            for date in dates {
                assert_eq!(date.len(), "2026-01-01T00:00:00.000Z".len());
                assert_eq!(&date[19..20], ".");
                assert!(date.ends_with('Z'));
            }

            for pair in dates.windows(2) {
                let previous = pair[0].parse::<Timestamp>().unwrap();
                let next = pair[1].parse::<Timestamp>().unwrap();

                assert_eq!(next.duration_since(previous), SignedDuration::from_secs(1));
            }

            previous_dates = playlists[0].clone();
        }
    }
}

#[test]
fn restart_checks_program_date_at_the_first_common_keyframe() {
    let fixture = Fixture::new();
    let open = || {
        let mut config = fixture.config(2, 10);
        config.program_date_time = true;

        Packager::new(
            config,
            Writer {
                fail_rendition: None,
            },
        )
        .unwrap()
    };
    // Opening the packager and receiving the first rendition still happen
    // before the old window ends. The shared keyframe arrives at its end.
    let old_start = Timestamp::from_second(Timestamp::now().as_second() + 60).unwrap();
    let old_end = old_start.checked_add(SignedDuration::from_secs(2)).unwrap();
    let mut first = open();

    feed_with_clock(&mut first, 2, 0, 50, || old_start).unwrap();
    first.finish().unwrap();
    drop(first);

    let path = fixture.0.join("v0.m3u8");
    let previous = fs::read_to_string(&path).unwrap();
    let mut resumed = open();

    resumed
        .write_with_clock(packet(0, 0, true), || {
            panic!("one rendition cannot establish the program date")
        })
        .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), previous);

    feed_with_clock(&mut resumed, 2, 1_000_000, 25, || old_end).unwrap();
    resumed.finish().unwrap();

    let text = fs::read_to_string(path).unwrap();

    assert!(text.contains("#EXT-X-MEDIA-SEQUENCE:0\n"));
    assert!(text.contains("v0_0.ts\n"));
    assert!(text.contains(&format!(
        "#EXT-X-PROGRAM-DATE-TIME:{}\n",
        old_end.strftime("%FT%T%.3fZ")
    )));
    assert_eq!(text.matches("#EXTINF:").count(), 3);
}

#[test]
fn discovered_codecs_are_published_per_variant_without_accepting_manifest_injection() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(2, 3),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    packager
        .set_codecs(0, "avc1.64001f,mp4a.40.2".into())
        .unwrap();
    assert!(
        packager
            .set_codecs(0, "avc1.64001f\"\ninvalid".into())
            .is_err()
    );
    assert!(packager.set_codecs(2, "avc1.64001f".into()).is_err());
    feed(&mut packager, 2, 0, 25).unwrap();
    packager.finish().unwrap();

    let master = fs::read_to_string(fixture.0.join("master.m3u8")).unwrap();
    let declarations: Vec<_> = master
        .lines()
        .filter(|line| line.starts_with("#EXT-X-STREAM-INF:"))
        .collect();

    assert!(declarations[0].contains("CODECS=\"avc1.64001f,mp4a.40.2\""));
    assert!(!declarations[1].contains("CODECS="));
}

#[test]
fn failed_segment_does_not_publish_a_partial_variant_set() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(2, 3),
        Writer {
            fail_rendition: Some(1),
        },
    )
    .unwrap();
    let result = feed(&mut packager, 2, 0, 26);

    assert!(result.is_err());
    assert!(!fixture.0.join("master.m3u8").exists());
    assert!(!fixture.0.join("v0.m3u8").exists());
    assert!(!fixture.0.join("v0_vtt.m3u8").exists());
}

#[test]
fn restart_preserves_media_and_uses_a_new_generation_without_reusing_orphan_names() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(2, 5),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 2, 0, 50).unwrap();
    packager.finish().unwrap();

    let old_segment = fs::read(fixture.0.join("v0_0.ts")).unwrap();

    fs::write(fixture.0.join("v0_99.ts"), "orphan").unwrap();
    drop(packager);

    let mut resumed = Packager::new(
        fixture.config(2, 5),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut resumed, 2, 0, 25).unwrap();
    resumed.finish().unwrap();

    let playlist = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();

    assert!(playlist.contains("v0_0.ts"));
    assert!(playlist.contains("#EXT-X-DISCONTINUITY\n"));
    assert!(playlist.contains("v0_100.ts"));
    assert_eq!(fs::read(fixture.0.join("v0_0.ts")).unwrap(), old_segment);
    assert!(fixture.0.join("v0_99.ts").exists());

    // The resource number may jump, while implicit MEDIA-SEQUENCE remains contiguous.
    drop(resumed);
    assert!(
        Packager::new(
            fixture.config(2, 5),
            Writer {
                fail_rendition: None
            }
        )
        .is_ok()
    );
}

#[test]
fn timestamp_map_wraps_on_the_33_bit_clock() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(1, 3),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();
    let base = ((1_i64 << 33) / 90 + 10) * 1000;

    packager
        .write_cue(Cue {
            start_ms: base / 1000 + 100,
            end_ms: base / 1000 + 900,
            text: "After wrap".into(),
        })
        .unwrap();
    feed(&mut packager, 1, base, 25).unwrap();
    packager.finish().unwrap();

    let subtitle = fs::read_to_string(fixture.0.join("v00.vtt")).unwrap();
    let expected = (base / 1000 * 90).rem_euclid(1_i64 << 33);

    assert!(subtitle.contains(&format!("MPEGTS:{expected}")));
    assert!(subtitle.contains("After wrap"));
}

#[test]
fn live_window_keeps_only_the_default_extra_segment() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(1, 2),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 1, 0, 275).unwrap();
    packager.finish().unwrap();

    let playlist = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();

    assert!(playlist.contains("#EXT-X-MEDIA-SEQUENCE:8"));
    assert!(!playlist.contains("v0_0.ts"));
    assert!(!fixture.0.join("v0_0.ts").exists());
    assert!(!fixture.0.join("v00.vtt").exists());
    assert!(fixture.0.join("v0_7.ts").exists());
    assert!(fixture.0.join("v07.vtt").exists());
}

#[test]
fn rejects_invalid_layout_and_bounds_packet_and_cue_storage() {
    let fixture = Fixture::new();
    let mut config = fixture.config(1, 2);

    config.renditions[0].name = "../escape".into();
    assert!(
        Packager::new(
            config,
            Writer {
                fail_rendition: None
            }
        )
        .is_err()
    );

    let mut packager = Packager::new(
        fixture.config(1, 2),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();
    let mut oversized = packet(0, 0, true);

    oversized.bytes = 65 * 1024 * 1024;
    assert!(packager.write(oversized).is_err());
    assert!(
        packager
            .write_cue(Cue {
                start_ms: 1,
                end_ms: 0,
                text: "invalid".into()
            })
            .is_err()
    );
}

#[test]
fn inconsistent_resume_is_rejected_without_overwriting_existing_playlists() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(2, 2),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 2, 0, 25).unwrap();
    packager.finish().unwrap();
    drop(packager);

    let previous = fs::read(fixture.0.join("v0.m3u8")).unwrap();
    let other = fs::read_to_string(fixture.0.join("v1.m3u8")).unwrap();

    fs::write(
        fixture.0.join("v1.m3u8"),
        other.replace("MEDIA-SEQUENCE:0", "MEDIA-SEQUENCE:1"),
    )
    .unwrap();

    assert!(
        Packager::new(
            fixture.config(2, 2),
            Writer {
                fail_rendition: None
            }
        )
        .is_err()
    );
    assert_eq!(fs::read(fixture.0.join("v0.m3u8")).unwrap(), previous);
}

#[test]
fn new_tracks_join_existing_window_and_resume_again() {
    let fixture = Fixture::new();
    let mut config = fixture.config(1, 5);
    config.subtitle = None;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 1, 0, 50).unwrap();
    packager.finish().unwrap();
    drop(packager);

    let previous = fs::read(fixture.0.join("v0_0.ts")).unwrap();
    let mut config = fixture.config(2, 5);
    config.renditions.reverse();
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 2, 0, 25).unwrap();
    packager.finish().unwrap();
    drop(packager);

    let video = fs::read_to_string(fixture.0.join("v1.m3u8")).unwrap();
    let subtitle = fs::read_to_string(fixture.0.join("v1_vtt.m3u8")).unwrap();

    assert!(video.contains("MEDIA-SEQUENCE:2"));
    assert!(subtitle.contains("MEDIA-SEQUENCE:2"));
    assert!(video.contains("#EXT-X-DISCONTINUITY\n"));
    assert_eq!(fs::read(fixture.0.join("v0_0.ts")).unwrap(), previous);

    let mut config = fixture.config(2, 5);
    config.renditions.reverse();
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 2, 0, 25).unwrap();
    packager.finish().unwrap();
}

#[test]
fn segment_target_allows_rounding_tolerance_without_a_reserve() {
    let fixture = Fixture::new();
    let mut config = fixture.config(1, 5);
    config.segment_seconds = 2;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    for frame in 0..=60 {
        packager
            .write(packet(0, frame * 40_000, matches!(frame, 0 | 40 | 60)))
            .unwrap();
        packager.write(packet(1, frame * 40_000, false)).unwrap();
    }

    let playlist = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();

    assert!(playlist.contains("#EXTINF:2.400000,"));
    assert!(playlist.contains("#EXT-X-TARGETDURATION:2"));
}

#[test]
fn custom_delete_threshold_applies_to_video_and_subtitles_with_a_one_hour_window() {
    let fixture = Fixture::new();
    let mut config = fixture.config(1, 600);
    config.segment_seconds = 6;
    config.delete_threshold = 3;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 1, 0, 605 * 150).unwrap();
    packager.finish().unwrap();

    for number in 0..5 {
        assert_eq!(
            fixture.0.join(format!("v0_{number}.ts")).exists(),
            number >= 2
        );
        assert_eq!(
            fixture.0.join(format!("v0{number}.vtt")).exists(),
            number >= 2
        );
    }

    let playlist = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();

    assert!(playlist.contains("MEDIA-SEQUENCE:5"));
    assert_eq!(playlist.matches("#EXTINF:").count(), 600);
}

#[test]
fn restart_prunes_excess_orphans_only_after_new_media_is_published() {
    let fixture = Fixture::new();

    for number in 0..4 {
        fs::write(fixture.0.join(format!("v0_{number}.ts")), "orphan").unwrap();
        fs::write(fixture.0.join(format!("v0{number}.vtt")), "orphan").unwrap();
    }

    let mut packager = Packager::new(
        fixture.config(1, 2),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    assert!(fixture.0.join("v0_0.ts").exists());
    feed(&mut packager, 1, 0, 25).unwrap();
    packager.finish().unwrap();

    for number in 0..4 {
        assert_eq!(
            fixture.0.join(format!("v0_{number}.ts")).exists(),
            number == 3
        );
        assert_eq!(
            fixture.0.join(format!("v0{number}.vtt")).exists(),
            number == 3
        );
    }

    assert!(fixture.0.join("v0_4.ts").exists());
    assert!(fixture.0.join("v04.vtt").exists());
}

#[test]
fn video_names_ending_in_vtt_do_not_own_other_subtitle_files() {
    let fixture = Fixture::new();
    let mut config = fixture.config(1, 2);
    config.renditions[0].name = "news_vtt".into();
    config.subtitle = None;

    fs::write(fixture.0.join("news0.vtt"), "foreign subtitles").unwrap();
    fs::write(fixture.0.join("news1.vtt"), "foreign subtitles").unwrap();
    fs::write(fixture.0.join("news_vtt_99.ts"), "orphan").unwrap();
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 1, 0, 275).unwrap();
    packager.finish().unwrap();

    assert!(fixture.0.join("news0.vtt").exists());
    assert!(fixture.0.join("news1.vtt").exists());
    assert!(!fixture.0.join("news_vtt_99.ts").exists());
}

#[test]
fn different_initial_scene_cuts_join_at_the_first_common_keyframe() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(2, 2),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    for frame in 40..100 {
        for rendition in 0..2 {
            let keyframe = if rendition == 0 {
                matches!(frame, 40 | 50 | 75)
            } else {
                matches!(frame, 50 | 75)
            };

            packager
                .write(packet(rendition * 2, frame * 40_000, keyframe))
                .unwrap();
            packager
                .write(packet(rendition * 2 + 1, frame * 40_000, false))
                .unwrap();
        }
    }

    packager.finish().unwrap();

    for rendition in 0..2 {
        let first = fs::read_to_string(fixture.0.join(format!("v{rendition}_0.ts"))).unwrap();

        assert_eq!(first, "2000000:25");
    }
}

#[test]
fn subtitles_can_be_disabled_and_reenabled_after_video_advances() {
    let fixture = Fixture::new();
    let mut packager = Packager::new(
        fixture.config(1, 3),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 1, 0, 50).unwrap();
    packager.finish().unwrap();
    drop(packager);

    let mut config = fixture.config(1, 3);
    config.subtitle = None;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 1, 0, 50).unwrap();
    packager.finish().unwrap();
    drop(packager);

    let previous = fs::read(fixture.0.join("v0_vtt.m3u8")).unwrap();
    let mut packager = Packager::new(
        fixture.config(1, 3),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    assert_eq!(fs::read(fixture.0.join("v0_vtt.m3u8")).unwrap(), previous);
    feed(&mut packager, 1, 0, 25).unwrap();
    packager.finish().unwrap();

    let subtitles = fs::read_to_string(fixture.0.join("v0_vtt.m3u8")).unwrap();
    let video = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();

    assert!(subtitles.contains("MEDIA-SEQUENCE:4"));
    assert!(subtitles.contains("DISCONTINUITY-SEQUENCE:1"));
    assert!(video.contains("v0_4.ts"));
    assert!(fixture.0.join("v04.vtt").exists());
}

#[test]
fn late_common_keyframes_catch_up_without_losing_media_or_captions() {
    use std::sync::{Arc, Mutex};

    struct RecordingWriter(Arc<Mutex<Vec<(usize, i64)>>>);

    impl SegmentWriter<()> for RecordingWriter {
        fn write_segment(
            &mut self,
            path: &Path,
            packets: &[MediaPacket<()>],
            rendition: usize,
        ) -> Result<()> {
            assert!(packets.is_sorted_by_key(|packet| (packet.dts_us, packet.stream)));

            let mut recorded = self.0.lock().unwrap();

            for packet in packets
                .iter()
                .filter(|packet| packet.stream / 2 == rendition)
            {
                recorded.push((packet.stream, packet.pts_us));
            }

            fs::write(path, "media")?;

            Ok(())
        }
    }

    let fixture = Fixture::new();
    let mut config = fixture.config(2, 100);
    config.segment_seconds = 6;
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let mut packager = Packager::new(config, RecordingWriter(recorded.clone())).unwrap();

    packager
        .write_cue(Cue {
            start_ms: 6200,
            end_ms: 6600,
            text: "Across delayed boundary".into(),
        })
        .unwrap();

    for frame in 0..750 {
        for rendition in 0..2 {
            // One rendition is late by 400 ms; common cuts must still agree.
            let keyframe = (frame % 50 == 0 && !(rendition == 1 && matches!(frame, 150 | 450)))
                || matches!(frame, 160 | 460);

            packager
                .write(packet(rendition * 2, frame * 40_000, keyframe))
                .unwrap();
            packager
                .write(packet(rendition * 2 + 1, frame * 40_000, false))
                .unwrap();
        }
    }

    packager.finish().unwrap();

    for rendition in 0..2 {
        let playlist = fs::read_to_string(fixture.0.join(format!("v{rendition}.m3u8"))).unwrap();
        let durations: Vec<f64> = playlist
            .lines()
            .filter_map(|line| line.strip_prefix("#EXTINF:"))
            .map(|value| value.trim_end_matches(',').parse().unwrap())
            .collect();

        assert_eq!(durations, [6.4, 5.6, 6.4, 5.6, 6.0]);
        assert!(playlist.contains("#EXT-X-TARGETDURATION:6"));
    }

    let recorded = recorded.lock().unwrap();

    for stream in 0..4 {
        let timestamps: Vec<_> = recorded
            .iter()
            .filter(|(track, _)| *track == stream)
            .map(|(_, pts)| *pts)
            .collect();

        assert_eq!(
            timestamps,
            (0..750).map(|frame| frame * 40_000).collect::<Vec<_>>(),
            "lost or duplicated packets in track {stream}"
        );
    }

    for number in 0..=1 {
        let text = fs::read_to_string(fixture.0.join(format!("v0{number}.vtt"))).unwrap();

        assert!(text.contains("Across delayed boundary"));
    }
}

#[test]
fn nonzero_session_origin_joins_the_program_grid_with_a_short_first_segment() {
    let fixture = Fixture::new();
    let mut config = fixture.config(1, 100);
    config.segment_seconds = 6;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    for frame in 40..650 {
        packager
            .write(packet(0, frame * 40_000, frame == 40 || frame % 50 == 0))
            .unwrap();
        packager.write(packet(1, frame * 40_000, false)).unwrap();
    }

    packager.finish().unwrap();

    let text = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();
    let durations: Vec<f64> = text
        .lines()
        .filter_map(|line| line.strip_prefix("#EXTINF:"))
        .map(|value| value.trim_end_matches(',').parse().unwrap())
        .collect();

    assert_eq!(durations, [4.4, 6.0, 6.0, 6.0, 2.0]);
    assert!(
        fs::read_to_string(fixture.0.join("v00.vtt"))
            .unwrap()
            .contains("LOCAL:00:00:01.600,MPEGTS:144000")
    );
}

#[test]
fn missing_keyframes_fail_before_publishing_an_overlong_segment() {
    let fixture = Fixture::new();
    let mut config = fixture.config(1, 10);
    config.segment_seconds = 2;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    for frame in 0..=150 {
        let result = packager.write(packet(0, frame * 40_000, matches!(frame, 0 | 150)));

        if frame == 150 {
            // Video keyframe has arrived, but matching audio progress is still pending.
            result.unwrap();
            assert!(
                packager
                    .write(packet(1, frame * 40_000, false))
                    .unwrap_err()
                    .to_string()
                    .contains("fixed target duration")
            );
        } else {
            result.unwrap();
            packager.write(packet(1, frame * 40_000, false)).unwrap();
        }
    }

    assert!(!fixture.0.join("v0_0.ts").exists());
    assert!(!fixture.0.join("v00.vtt").exists());
    assert!(!fixture.0.join("v0.m3u8").exists());
    assert!(!fixture.0.join("master.m3u8").exists());
}

#[test]
fn short_live_windows_keep_three_target_durations_for_video_and_subtitles() {
    for limit in [1, 3] {
        let fixture = Fixture::new();
        let mut config = fixture.config(1, limit);
        config.segment_seconds = 6;
        let mut packager = Packager::new(
            config,
            Writer {
                fail_rendition: None,
            },
        )
        .unwrap();

        for frame in 0..=1800 {
            let pts = frame * 40_000;
            let key = frame % 50 == 0;

            packager.write(packet(0, pts, key)).unwrap();
            packager.write(packet(1, pts, false)).unwrap();
        }

        for name in ["v0.m3u8", "v0_vtt.m3u8"] {
            let text = fs::read_to_string(fixture.0.join(name)).unwrap();
            let durations: Vec<f64> = text
                .lines()
                .filter_map(|line| line.strip_prefix("#EXTINF:"))
                .map(|value| value.trim_end_matches(',').parse().unwrap())
                .collect();

            assert!(text.contains("#EXT-X-TARGETDURATION:6\n"));
            assert!(!text.contains("#EXT-X-MEDIA-SEQUENCE:0\n"));
            assert!(durations.len() >= limit as usize);
            assert!(durations.iter().sum::<f64>() >= 18.0);
            assert!(durations.iter().skip(1).sum::<f64>() < 18.0);
        }
    }
}

#[test]
fn resume_preserves_target_duration_and_rejects_an_increase_without_deleting_media() {
    let fixture = Fixture::new();
    let mut config = fixture.config(1, 5);
    config.segment_seconds = 6;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 1, 0, 150).unwrap();
    packager.finish().unwrap();
    drop(packager);

    let playlist = fixture.0.join("v0.m3u8");
    let original = fs::read(&playlist).unwrap();
    let media = fs::read(fixture.0.join("v0_0.ts")).unwrap();
    let mut larger = fixture.config(1, 5);
    larger.segment_seconds = 10;

    let result = Packager::new(
        larger,
        Writer {
            fail_rendition: None,
        },
    );

    assert!(
        result
            .err()
            .unwrap()
            .to_string()
            .contains("fresh HLS history")
    );
    assert_eq!(fs::read(&playlist).unwrap(), original);
    assert_eq!(fs::read(fixture.0.join("v0_0.ts")).unwrap(), media);

    let mut smaller = fixture.config(1, 5);
    smaller.segment_seconds = 4;
    let mut resumed = Packager::new(
        smaller,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut resumed, 1, 0, 100).unwrap();
    resumed.finish().unwrap();

    let text = fs::read_to_string(&playlist).unwrap();

    assert!(text.contains("#EXT-X-TARGETDURATION:6\n"));
    assert!(text.contains("v0_0.ts"));
    assert!(text.contains("#EXTINF:4.000000,"));

    // An existing playlist from the previous implementation retains its
    // published target, even though fresh six-second outputs now use six.
    for name in ["v0.m3u8", "v0_vtt.m3u8"] {
        let path = fixture.0.join(name);
        let text = fs::read_to_string(&path).unwrap();

        fs::write(
            path,
            text.replace("#EXT-X-TARGETDURATION:6", "#EXT-X-TARGETDURATION:8"),
        )
        .unwrap();
    }

    let mut config = fixture.config(1, 5);
    config.segment_seconds = 6;
    let mut resumed = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut resumed, 1, 0, 150).unwrap();
    resumed.finish().unwrap();

    let text = fs::read_to_string(&playlist).unwrap();

    assert!(text.contains("#EXT-X-TARGETDURATION:8\n"));
    assert!(text.contains("v0_0.ts"));
}

#[test]
fn rendition_suffix_only_collides_when_subtitles_are_enabled() {
    let fixture = Fixture::new();
    let mut config = fixture.config(2, 5);
    config.renditions[1].name = "v0_vtt".into();

    assert!(
        Packager::new(
            config,
            Writer {
                fail_rendition: None,
            }
        )
        .is_err()
    );

    let mut config = fixture.config(2, 5);
    config.renditions[1].name = "v0_vtt".into();
    config.subtitle = None;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 2, 0, 50).unwrap();
    packager.finish().unwrap();

    assert!(fixture.0.join("v0.m3u8").exists());
    assert!(fixture.0.join("v0_vtt.m3u8").exists());
    assert!(fixture.0.join("v0_vtt_0.ts").exists());
}

#[test]
fn resumed_codec_declarations_cover_retained_media_until_it_expires() {
    const CURRENT: &str = "avc1.64001f,mp4a.40.2";

    for previous in [
        CURRENT,
        "avc1.42001e,mp4a.40.2",
        "avc1.64001f,mp4a.40.5",
        "",
    ] {
        for delayed in [false, true] {
            let fixture = Fixture::new();
            let mut config = fixture.config(2, 3);
            config.renditions[0].codecs = previous.into();
            config.renditions[1].codecs = CURRENT.into();
            let mut packager = Packager::new(
                config,
                Writer {
                    fail_rendition: None,
                },
            )
            .unwrap();

            feed(&mut packager, 2, 0, 100).unwrap();
            packager.finish().unwrap();

            let old_playlist = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();
            let old_segments: Vec<_> = old_playlist
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
                .collect();
            let mut config = fixture.config(2, 3);
            config.renditions[0].codecs = if delayed {
                String::new()
            } else {
                CURRENT.into()
            };
            config.renditions[1].codecs = CURRENT.into();
            let mut resumed = Packager::new(
                config,
                Writer {
                    fail_rendition: None,
                },
            )
            .unwrap();

            if delayed {
                resumed.set_codecs(0, CURRENT.into()).unwrap();
            }

            feed(&mut resumed, 2, 0, 50).unwrap();

            let playlist = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();
            let master = fs::read_to_string(fixture.0.join("master.m3u8")).unwrap();
            let declarations: Vec<_> = master
                .lines()
                .filter(|line| line.starts_with("#EXT-X-STREAM-INF:"))
                .collect();

            assert!(
                old_segments
                    .iter()
                    .any(|uri| playlist.lines().any(|line| line == uri))
            );
            assert!(old_segments.iter().all(|uri| fixture.0.join(uri).exists()));
            assert_eq!(
                declarations[0].contains("CODECS="),
                previous == CURRENT,
                "previous={previous}, delayed={delayed}: {master}"
            );
            assert!(declarations[1].contains(CURRENT));
            resumed.finish().unwrap();

            // A second restart must not turn an omitted declaration into a
            // claim about the still-retained mixed window.
            let mut config = fixture.config(2, 3);
            config.renditions[0].codecs = CURRENT.into();
            config.renditions[1].codecs = CURRENT.into();
            let mut resumed = Packager::new(
                config,
                Writer {
                    fail_rendition: None,
                },
            )
            .unwrap();

            feed(&mut resumed, 2, 0, 50).unwrap();

            let master = fs::read_to_string(fixture.0.join("master.m3u8")).unwrap();
            let first = master
                .lines()
                .find(|line| line.starts_with("#EXT-X-STREAM-INF:"))
                .unwrap();

            assert_eq!(first.contains("CODECS="), previous == CURRENT);
            feed(&mut resumed, 2, 2_000_000, 250).unwrap();
            resumed.finish().unwrap();

            let playlist = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();
            let master = fs::read_to_string(fixture.0.join("master.m3u8")).unwrap();

            assert!(
                !old_segments
                    .iter()
                    .any(|uri| playlist.lines().any(|line| line == uri))
            );
            assert_eq!(master.matches(CURRENT).count(), 2);
        }
    }
}

#[test]
fn fixed_target_accepts_rounded_durations_but_rejects_half_second_overruns() {
    for duration in [6_480_000, 6_499_999, 6_500_000, 6_520_000] {
        let fixture = Fixture::new();
        let mut config = fixture.config(2, 10);
        config.segment_seconds = 6;
        let mut packager = Packager::new(
            config,
            Writer {
                fail_rendition: None,
            },
        )
        .unwrap();

        for rendition in 0..2 {
            packager.write(packet(rendition * 2, 0, true)).unwrap();
            packager.write(packet(rendition * 2 + 1, 0, false)).unwrap();
            packager
                .write(packet(rendition * 2, duration, true))
                .unwrap();
        }

        packager.write(packet(1, duration, false)).unwrap();

        let result = packager.write(packet(3, duration, false));

        if duration < 6_500_000 {
            result.unwrap();

            let video = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();
            let subtitles = fs::read_to_string(fixture.0.join("v0_vtt.m3u8")).unwrap();

            assert!(video.contains("#EXT-X-TARGETDURATION:6\n"));
            assert!(subtitles.contains("#EXT-X-TARGETDURATION:6\n"));
            assert!(video.contains(&format!("#EXTINF:{:.6},", duration as f64 / 1_000_000.0)));
        } else {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("fixed target duration")
            );
            assert!(!fixture.0.join("v0_0.ts").exists());
            assert!(!fixture.0.join("master.m3u8").exists());
        }
    }
}

#[test]
fn late_overrun_preserves_published_history_and_can_resume() {
    let fixture = Fixture::new();
    let mut config = fixture.config(2, 10);
    config.segment_seconds = 6;
    let mut packager = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut packager, 2, 0, 151).unwrap();

    let paths = [
        "master.m3u8",
        "v0.m3u8",
        "v1.m3u8",
        "v0_vtt.m3u8",
        "v0_0.ts",
        "v1_0.ts",
        "v00.vtt",
    ];
    let before: Vec<_> = paths
        .iter()
        .map(|name| fs::read(fixture.0.join(name)).unwrap())
        .collect();

    for stream in 0..4 {
        let result = packager.write(packet(stream, 12_500_000, stream % 2 == 0));

        if stream == 3 {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("fixed target duration")
            );
        } else {
            result.unwrap();
        }
    }

    for (name, bytes) in paths.iter().zip(&before) {
        assert_eq!(fs::read(fixture.0.join(name)).unwrap(), *bytes);
    }

    assert!(!fixture.0.join("v0_1.ts").exists());
    assert!(!fixture.0.join("v1_1.ts").exists());
    assert!(!fixture.0.join("v01.vtt").exists());
    drop(packager);

    let mut config = fixture.config(2, 10);
    config.segment_seconds = 6;
    let mut resumed = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut resumed, 2, 0, 151).unwrap();
    resumed.finish().unwrap();

    for name in ["v0.m3u8", "v1.m3u8", "v0_vtt.m3u8"] {
        let text = fs::read_to_string(fixture.0.join(name)).unwrap();

        assert!(text.contains("#EXT-X-TARGETDURATION:6\n"));
        assert!(text.contains("#EXT-X-DISCONTINUITY\n"));
    }

    assert_eq!(fs::read(fixture.0.join("v0_0.ts")).unwrap(), before[4]);
    assert!(fixture.0.join("v0_1.ts").exists());
}

// Fail through the existing container-writer boundary after a playable history
// exists. Filesystem obstructions exercise the real atomic publication code.
struct PublicationFault {
    temporary_playlist: Option<&'static str>,
    disk_full: bool,
}

impl SegmentWriter<()> for PublicationFault {
    fn write_segment(
        &mut self,
        path: &Path,
        packets: &[MediaPacket<()>],
        rendition: usize,
    ) -> Result<()> {
        if path.file_name().unwrap() == format!("v{rendition}_1.ts").as_str() {
            if self.disk_full {
                return Err(std::io::Error::from(std::io::ErrorKind::StorageFull).into());
            }

            if rendition == 1
                && let Some(name) = self.temporary_playlist
            {
                fs::create_dir(path.parent().unwrap().join(name))?;
            }
        }

        Writer {
            fail_rendition: None,
        }
        .write_segment(path, packets, rendition)
    }
}

#[test]
fn storage_full_preserves_playable_history_and_resume_skips_orphans() {
    let fixture = Fixture::new();
    let config = fixture.config(2, 3);
    let mut packager = Packager::new(
        fixture.config(2, 3),
        PublicationFault {
            temporary_playlist: None,
            disk_full: true,
        },
    )
    .unwrap();

    feed(&mut packager, 2, 0, 26).unwrap();

    let previous = fs::read(fixture.0.join("v0.m3u8")).unwrap();
    let error = feed(&mut packager, 2, 1_040_000, 26).unwrap_err();

    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::StorageFull)
    }));
    assert_eq!(fs::read(fixture.0.join("v0.m3u8")).unwrap(), previous);
    drop(packager);

    let mut resumed = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut resumed, 2, 3_000_000, 26).unwrap();
    assert!(
        fs::read_to_string(fixture.0.join("v0.m3u8"))
            .unwrap()
            .contains("#EXT-X-DISCONTINUITY\n")
    );
    assert_eq!(
        fs::read_to_string(fixture.0.join("v0_0.ts")).unwrap(),
        "0:25"
    );
}

#[test]
fn publication_failure_after_media_preserves_history_and_never_reuses_orphans() {
    let fixture = Fixture::new();
    let config = fixture.config(2, 3);
    let blocked = ".v0_vtt.m3u8.native.tmp";
    let mut packager = Packager::new(
        fixture.config(2, 3),
        PublicationFault {
            temporary_playlist: Some(blocked),
            disk_full: false,
        },
    )
    .unwrap();

    feed(&mut packager, 2, 0, 26).unwrap();

    let previous = fs::read(fixture.0.join("v0.m3u8")).unwrap();

    assert!(feed(&mut packager, 2, 1_040_000, 26).is_err());
    drop(packager);
    assert_eq!(fs::read(fixture.0.join("v0.m3u8")).unwrap(), previous);
    assert!(fixture.0.join("v0_1.ts").is_file());
    assert!(fixture.0.join("v1_1.ts").is_file());

    let orphan = fs::read(fixture.0.join("v0_1.ts")).unwrap();

    fs::remove_dir(fixture.0.join(blocked)).unwrap();

    let mut resumed = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut resumed, 2, 3_000_000, 26).unwrap();
    assert!(fixture.0.join("v0_2.ts").is_file());
    assert_eq!(fs::read(fixture.0.join("v0_1.ts")).unwrap(), orphan);
}

#[test]
fn interrupted_variant_publication_resumes_beyond_all_published_sequences() {
    const CHILD_DIRECTORY: &str = "FF_HLS_PARTIAL_MANIFEST_FIXTURE";
    let blocked = ".v1.m3u8.native.tmp";
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let fixture = Fixture(PathBuf::from(directory));
        let mut packager = Packager::new(
            fixture.config(2, 3),
            PublicationFault {
                temporary_playlist: Some(blocked),
                disk_full: false,
            },
        )
        .unwrap();
        feed(&mut packager, 2, 0, 26).unwrap();
        assert!(feed(&mut packager, 2, 1_040_000, 26).is_err());
        std::process::exit(74);
    }

    let fixture = Fixture::new();
    let config = fixture.config(2, 3);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "packager::tests::interrupted_variant_publication_resumes_beyond_all_published_sequences"])
        .env(CHILD_DIRECTORY, &fixture.0).output().unwrap();
    assert_eq!(child.status.code(), Some(74), "{child:?}");
    assert!(
        fs::read_to_string(fixture.0.join("v0.m3u8"))
            .unwrap()
            .contains("v0_1.ts")
    );
    assert!(
        !fs::read_to_string(fixture.0.join("v1.m3u8"))
            .unwrap()
            .contains("v1_1.ts")
    );
    fs::remove_dir(fixture.0.join(blocked)).unwrap();

    let snapshot: Vec<_> = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let contents = fs::read(&path).unwrap();

            (path, contents)
        })
        .collect();
    let mut resumed = Packager::new(
        config,
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();
    for (path, contents) in snapshot {
        assert_eq!(fs::read(path).unwrap(), contents);
    }
    feed(&mut resumed, 2, 3_000_000, 26).unwrap();
    for (name, resource) in [("v0", "v0_2.ts"), ("v1", "v1_2.ts"), ("v0_vtt", "v02.vtt")] {
        let playlist = fs::read_to_string(fixture.0.join(format!("{name}.m3u8"))).unwrap();
        assert!(playlist.contains("#EXT-X-DISCONTINUITY\n"));
        assert!(playlist.contains(resource));
    }
}

#[test]
fn abrupt_process_exit_after_media_resumes_without_overwriting_history() {
    const CHILD_DIRECTORY: &str = "FF_HLS_ABORT_FIXTURE";

    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        struct ExitAfterMedia;

        impl SegmentWriter<()> for ExitAfterMedia {
            fn write_segment(
                &mut self,
                path: &Path,
                packets: &[MediaPacket<()>],
                rendition: usize,
            ) -> Result<()> {
                Writer {
                    fail_rendition: None,
                }
                .write_segment(path, packets, rendition)?;

                if path.file_name().unwrap() == "v1_1.ts" {
                    // Exit without running destructors or publishing the new manifests.
                    std::process::exit(73);
                }

                Ok(())
            }
        }

        let fixture = Fixture(PathBuf::from(directory));
        let mut packager = Packager::new(fixture.config(2, 3), ExitAfterMedia).unwrap();

        feed(&mut packager, 2, 0, 51).unwrap();
        panic!("child did not exit at the publication boundary");
    }

    let fixture = Fixture::new();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "packager::tests::abrupt_process_exit_after_media_resumes_without_overwriting_history",
        ])
        .env(CHILD_DIRECTORY, &fixture.0)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(73), "{output:?}");

    let history = fs::read(fixture.0.join("v0_0.ts")).unwrap();
    let orphan = fs::read(fixture.0.join("v0_1.ts")).unwrap();
    let mut resumed = Packager::new(
        fixture.config(2, 3),
        Writer {
            fail_rendition: None,
        },
    )
    .unwrap();

    feed(&mut resumed, 2, 3_000_000, 26).unwrap();
    assert_eq!(fs::read(fixture.0.join("v0_0.ts")).unwrap(), history);
    assert_eq!(fs::read(fixture.0.join("v0_1.ts")).unwrap(), orphan);
    assert!(fixture.0.join("v0_2.ts").is_file());
}
