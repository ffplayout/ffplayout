use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Result, bail};

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
            program_date_time: true,
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

fn feed(
    packager: &mut Packager<(), Writer>,
    renditions: usize,
    base: i64,
    frames: i64,
) -> Result<()> {
    for frame in 0..frames {
        for rendition in 0..renditions {
            packager.write(packet(
                rendition * 2,
                base + frame * 40_000,
                frame % 25 == 0,
            ))?;
            packager.write(packet(rendition * 2 + 1, base + frame * 40_000, false))?;
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

    assert!(playlist.contains("#EXT-X-MEDIA-SEQUENCE:2"));
    assert!(!playlist.contains("v0_0.ts"));
    assert!(!fixture.0.join("v0_0.ts").exists());
    assert!(!fixture.0.join("v00.vtt").exists());
    assert!(fixture.0.join("v0_1.ts").exists());
    assert!(fixture.0.join("v01.vtt").exists());
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
fn segment_target_allows_a_two_second_keyframe_grid_offset() {
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

    for frame in 0..=90 {
        packager
            .write(packet(0, frame * 40_000, matches!(frame, 0 | 40 | 90)))
            .unwrap();
        packager.write(packet(1, frame * 40_000, false)).unwrap();
    }

    let playlist = fs::read_to_string(fixture.0.join("v0.m3u8")).unwrap();

    assert!(playlist.contains("#EXTINF:3.600000,"));
    assert!(playlist.contains("#EXT-X-TARGETDURATION:4"));
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
            start_ms: 7500,
            end_ms: 8500,
            text: "Across delayed boundary".into(),
        })
        .unwrap();

    for frame in 0..750 {
        for rendition in 0..2 {
            // Only the second rendition misses the nominal cuts at 6 and 18 s.
            let keyframe = frame % 50 == 0 && !(rendition == 1 && matches!(frame, 150 | 450));

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

        assert_eq!(durations, [8.0, 4.0, 8.0, 4.0, 6.0]);
        assert!(playlist.contains("#EXT-X-TARGETDURATION:8"));
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
fn nonzero_session_origin_keeps_relative_targets_after_a_delayed_first_cut() {
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

    assert_eq!(durations, [6.4, 6.0, 6.0, 6.0]);
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
            let key = frame % 50 == 0 && ![6_000_000, 18_000_000].contains(&pts);

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

            assert!(text.contains("#EXT-X-TARGETDURATION:8\n"));
            assert!(!text.contains("#EXT-X-MEDIA-SEQUENCE:0\n"));
            assert!(durations.len() > limit as usize);
            assert!(durations.iter().sum::<f64>() >= 24.0);
            assert!(durations.iter().skip(1).sum::<f64>() < 24.0);
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

    assert!(text.contains("#EXT-X-TARGETDURATION:8\n"));
    assert!(text.contains("v0_0.ts"));
    assert!(text.contains("#EXTINF:4.000000,"));
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
