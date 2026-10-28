//! Manual end-to-end recovery test: FFmpeg publisher -> RTMP decoder -> HLS.
use std::{
    collections::BTreeMap,
    fs,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};

use super::LiveOverrideOutput;
use crate::{
    HlsSubtitle, HlsVariant, LogoConfig, OutputConfig, PlaybackControl,
    audio_mixer::{
        LiveLoudnessConfig, LiveLoudnessControl, LoudnessScope,
        program::{ProgramAudioOutput, ProgramAudioState},
    },
    input::live::{LiveIdleTimeout, LiveInputBackend, LiveListenerConfig, spawn_live_listeners},
    output::{
        FrameOutput, Output,
        parallel::{OutputDestination, OutputTarget},
    },
};

#[derive(Debug, Default)]
struct PacketCapture(Mutex<BTreeMap<usize, (usize, i64)>>);

impl crate::OutputEndpoint for Arc<PacketCapture> {
    fn connect_packets(
        &self,
        streams: &[crate::EncodedStream],
    ) -> Result<Box<dyn crate::PacketSink>> {
        ensure!(
            streams.len() == 2 && streams[0].codec == "h264" && streams[1].codec == "aac",
            "unexpected native stream codecs"
        );
        Ok(Box::new(self.clone()))
    }
}

impl crate::PacketSink for Arc<PacketCapture> {
    fn write(&mut self, frame: crate::EncodedFrame<'_>) -> Result<()> {
        let mut tracks = self.0.lock().unwrap();
        if let Some((_, previous)) = tracks.get(&frame.stream) {
            ensure!(frame.dts_us > *previous, "non-monotonic native DTS");
        }
        let track = tracks.entry(frame.stream).or_default();
        track.0 += 1;
        track.1 = frame.dts_us;
        ensure!(!frame.data.is_empty(), "empty native access unit");
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        Ok(())
    }
}

struct Publisher(Child);
impl Drop for Publisher {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Publisher {
    fn signal(&self, name: &str) -> Result<()> {
        ensure!(
            Command::new("kill")
                .args([name, &self.0.id().to_string()])
                .status()?
                .success(),
            "publisher signal failed"
        );
        Ok(())
    }
}

fn publisher_wait(duration: Duration, cancel: &AtomicBool) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if cancel.load(Ordering::Acquire) {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
    true
}

fn sync_marker_sources() -> (String, String) {
    // Unequal intervals prevent a whole-period shift from looking synchronized.
    // No marker lies inside the intentional video-only gap at 6..7.8 seconds.
    let pulses = [1.0, 2.3, 4.1, 8.5, 10.2, 12.6, 15.1]
        .into_iter()
        .map(|time| format!("between(t,{time:.3},{:.3})", time + 0.1))
        .collect::<Vec<_>>()
        .join("+");

    (
        format!(
            "color=c=black:size=1280x720:rate=30,drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='{pulses}'"
        ),
        format!("aevalsrc='if({pulses},0.2*sin(2*PI*1000*t),0)':s=48000"),
    )
}

fn verify_sync_markers(log: &str) -> Result<()> {
    let mut video_time = None;
    let mut was_bright = false;
    let mut flashes = Vec::<f64>::new();
    let mut tones = Vec::<f64>::new();
    let mut pending_tone = None;
    for line in log.lines() {
        if let Some(time) = line
            .split("pts_time:")
            .nth(1)
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok())
        {
            video_time = Some(time);
        }
        if let Some(level) = line
            .split("lavfi.signalstats.YAVG=")
            .nth(1)
            .and_then(|value| value.trim().parse::<f64>().ok())
        {
            let bright = level > 180.0;
            if bright && !was_bright {
                flashes.push(
                    video_time.ok_or_else(|| anyhow::anyhow!("flash has no presentation time"))?,
                );
            }
            was_bright = bright;
        }
        if let Some(time) = line
            .split("silence_end:")
            .nth(1)
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<f64>().ok())
        {
            pending_tone = Some(time);
        }

        // silencedetect also emits silence_end at EOF without a tone.
        // A marker is a complete pulse followed by another silence interval.
        if line.contains("silence_start:")
            && let Some(time) = pending_tone.take()
        {
            tones.push(time);
        }
    }
    // The fixture starts with silence/black, so the first onset is present in
    // both tracks. Pair by ordinal position; never skip unmatched markers or
    // match a shifted flash to the next tone.
    ensure!(
        flashes.len() >= 3 && flashes.len() == tones.len(),
        "missing or extra sync markers: flashes={flashes:?}, tones={tones:?}"
    );

    for (index, (flash, tone)) in flashes.iter().zip(&tones).enumerate() {
        let distance = (tone - flash).abs();
        ensure!(
            distance <= 0.15,
            "sync marker {index}: flash={flash:.3}s tone={tone:.3}s differ by {distance:.3}s"
        );
    }

    eprintln!(
        "Verified {} ordered flash/tone pairs within 150 ms",
        flashes.len()
    );
    Ok(())
}

#[test]
fn decoded_marker_check_rejects_offsets_including_a_whole_marker_period() {
    let make_log = |audio_offset: f64| {
        [3.0, 6.0, 9.0, 12.0]
            .into_iter()
            .map(|time| {
                format!(
                    "frame:0 pts_time:{}\nlavfi.signalstats.YAVG=16\n\
                     frame:1 pts_time:{time}\nlavfi.signalstats.YAVG=235\n\
                     silence_end: {}\nsilence_start: {}\n",
                    time - 0.04,
                    time + audio_offset,
                    time + audio_offset + 0.1
                )
            })
            .collect::<String>()
    };

    assert!(verify_sync_markers(&make_log(0.02)).is_ok());
    assert!(verify_sync_markers(&format!("{}silence_end: 20.0\n", make_log(0.02))).is_ok());
    for offset in [-3.0, -1.96, 1.96, 3.0] {
        assert!(
            verify_sync_markers(&make_log(offset)).is_err(),
            "offset={offset}"
        );
    }

    let missing_flash =
        make_log(0.02).replacen("lavfi.signalstats.YAVG=235", "lavfi.signalstats.YAVG=16", 1);
    assert!(verify_sync_markers(&missing_flash).is_err());
}

fn verify_media(path: &Path, minimum_packets: usize, sync_markers: bool) -> Result<()> {
    let mut input = ffmpeg_next::format::input(path)?;
    let video = input
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .ok_or_else(|| anyhow::anyhow!("{}: missing video", path.display()))?
        .index();
    let audio = input
        .streams()
        .best(ffmpeg_next::media::Type::Audio)
        .ok_or_else(|| anyhow::anyhow!("{}: missing audio", path.display()))?
        .index();
    let mut previous_dts = BTreeMap::new();
    let mut counts = BTreeMap::<usize, usize>::new();

    loop {
        let mut packet = ffmpeg_next::Packet::empty();
        match packet.read(&mut input) {
            Ok(()) => {}
            Err(ffmpeg_next::Error::Eof) => break,
            Err(error) => return Err(error.into()),
        }
        let index = packet.stream();
        if index != video && index != audio {
            continue;
        }

        let dts = packet
            .dts()
            .ok_or_else(|| anyhow::anyhow!("{}: missing DTS in stream {index}", path.display()))?;
        if let Some(previous) = previous_dts.insert(index, dts) {
            ensure!(
                dts > previous,
                "{}: non-monotonic DTS in stream {index}: {previous} -> {dts}",
                path.display()
            );
        }

        *counts.entry(index).or_default() += 1;
    }

    for index in [video, audio] {
        ensure!(
            counts.get(&index).copied().unwrap_or_default() >= minimum_packets,
            "{}: too few packets in stream {index}: {counts:?}",
            path.display()
        );
    }

    // Decode both tracks, including delayed frames at EOF. Explicit maps make
    // missing tracks fatal; -xerror also rejects otherwise recoverable errors.
    let mut command = Command::new("ffmpeg");
    command
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            if sync_markers { "info" } else { "error" },
            "-xerror",
            "-err_detect",
            "explode",
            "-i",
        ])
        .arg(path)
        .args(["-map", "0:v:0", "-map", "0:a:0"]);
    if sync_markers {
        command.args([
            "-vf",
            "signalstats,metadata=mode=print:key=lavfi.signalstats.YAVG",
            "-af",
            "silencedetect=noise=-35dB:d=0.05",
        ]);
    }
    let decoded = command.args(["-f", "null", "-"]).output()?;
    ensure!(
        decoded.status.success(),
        "{}: decoding failed: {}",
        path.display(),
        String::from_utf8_lossy(&decoded.stderr)
    );
    if sync_markers {
        verify_sync_markers(&String::from_utf8_lossy(&decoded.stderr))?;
    }

    Ok(())
}

#[test]
#[ignore = "requires FFmpeg CLI, local TCP and about 90 seconds; run alone"]
fn rtmp_reception_pauses_and_reconnects_feed_three_hls_variants() -> Result<()> {
    let _ = env_logger::try_init();
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../tmp/rtmp-hls-recovery-{}",
        std::process::id()
    ));
    fs::create_dir_all(&directory)?;
    let socket = TcpListener::bind("127.0.0.1:0")?;
    let port = socket.local_addr()?.port();
    drop(socket);
    let url = format!("rtmp://127.0.0.1:{port}/live/recovery");
    let mut config = OutputConfig::new(1280, 720, 25, 48_000);
    crate::init_ffmpeg(&config)?;
    if ffmpeg_next::encoder::find_by_name("libfdk_aac").is_some() {
        config.audio_codec = "libfdk_aac".into();
    }
    eprintln!(
        "RTMP recovery artifacts: {}; audio encoder: {}",
        directory.display(),
        config.audio_codec
    );
    config
        .video_options
        .insert("preset".into(), "faster".into());
    config.video_options.insert("quality".into(), "23".into());
    let server_profile = std::env::var_os("FFPLAYOUT_RTMP_SERVER_PROFILE").is_some();
    if server_profile {
        config.output_timing_diagnostics = true;
        config.video_options.insert("maxrate".into(), "2400".into());
        config
            .video_options
            .insert("tune".into(), "zerolatency".into());
        config.loudness_scope = LoudnessScope::All;
        config.live_loudness_control = LiveLoudnessControl::new(
            true,
            LiveLoudnessConfig {
                target_lufs: -17.0,
                dead_band_lu: 1.0,
                max_gain_db: 8.0,
                max_attenuation_db: -12.0,
                gain_up_db_per_second: 0.5,
                gain_down_db_per_second: 2.0,
                silence_gate_lufs: -60.0,
                true_peak_ceiling_dbtp: -1.0,
                ..LiveLoudnessConfig::default()
            },
        );
        config.logo = Some(LogoConfig {
            path: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests_assets/storage/logo.png")
                .to_string_lossy()
                .into_owned(),
            scale: None,
            opacity: 0.7,
            position: "W-w-12:12".into(),
        });
    }
    config.destinations.push(OutputDestination {
        id: 5,
        config: config.clone(),
        target: OutputTarget::Hls {
            playlist: directory.join("master.m3u8").to_string_lossy().into_owned(),
            variants: [
                (1280, 720),
                (854, 480),
                if server_profile {
                    (426, 240)
                } else {
                    (480, 270)
                },
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (width, height))| HlsVariant {
                name: format!("v{i}"),
                width,
                height,
                video_bitrate: if server_profile {
                    [2_400_000, 1_200_000, 400_000][i]
                } else {
                    2_000_000
                },
                audio_bitrate: if server_profile && i == 2 {
                    64_000
                } else {
                    128_000
                },
            })
            .collect(),
            subtitle: server_profile.then(|| HlsSubtitle {
                name: "Deutsch".into(),
                language: "de-DE".into(),
                default: false,
            }),
            segment_seconds: if server_profile { 6 } else { 1 },
            list_size: if server_profile { 600 } else { 120 },
        },
    });
    let mut independent = config.clone();
    independent.destinations.clear();
    independent.width = 320;
    independent.height = 180;
    independent.stream_type = crate::StreamType::Custom;
    independent.stream_format = "mpegts".into();
    independent
        .video_options
        .insert("preset".into(), "ultrafast".into());
    let native = Arc::new(PacketCapture::default());
    if server_profile {
        // Same packet API and pacing as MoQ, without starting a public relay.
        let mut shared = config.clone();
        shared.destinations.clear();
        shared.encoding_source = Some((5, 1));
        shared.stream_endpoint = Some(Arc::new(native.clone()));
        config.destinations.push(OutputDestination {
            id: 6,
            config: shared,
            target: OutputTarget::Stream { url: String::new() },
        });
        independent = config.clone();
        independent.destinations.clear();
        independent.encoding_source = Some((5, 0));
        independent.stream_type = crate::StreamType::Custom;
        independent.stream_format = "flv".into();
    }
    config.destinations.push(OutputDestination {
        id: if server_profile { 7 } else { 6 },
        config: independent,
        target: OutputTarget::Stream {
            url: directory
                .join(if server_profile {
                    "shared.flv"
                } else {
                    "independent.ts"
                })
                .to_string_lossy()
                .into_owned(),
        },
    });
    let status = config.output_status.clone();
    let mut output = Output::open_parallel(&config)?;
    let mut audio_state = ProgramAudioState::default();
    let mut program_output = ProgramAudioOutput::new(&mut output, &mut audio_state, &config)?;
    if server_profile {
        let vtt = directory.join("captions.vtt");
        fs::write(
            &vtt,
            "WEBVTT\n\n00:00:00.000 --> 00:00:10.000\nRecovery test\n",
        )?;
        program_output.write_vtt_subtitles(vtt.to_str().unwrap(), 0, 0)?;
    }
    let mut live = spawn_live_listeners(
        vec![LiveListenerConfig {
            id: 1,
            priority: 0,
            idle_timeout: LiveIdleTimeout::new(4000)?,
            backend: LiveInputBackend::Rtmp,
            url: url.clone(),
            options: BTreeMap::new(),
            demuxer_options: BTreeMap::new(),
        }],
        config,
    );
    let control = PlaybackControl::default();
    let sync_markers = std::env::var_os("FFPLAYOUT_RTMP_SYNC_MARKERS").is_some();
    let mut completed_cases = 0;
    thread::sleep(Duration::from_millis(300));
    for (case, (full_range, clock_jump, resize)) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (false, false, true),
    ]
    .into_iter()
    .enumerate()
    {
        if sync_markers && case != 0 {
            continue;
        }
        if let Ok(selected) = std::env::var("FFPLAYOUT_RTMP_CASE")
            && selected.parse::<usize>()? != case
        {
            continue;
        }
        if std::env::var_os("FFPLAYOUT_RTMP_RESIZE_ONLY").is_some() && !resize {
            continue;
        }
        if resize {
            for (name, size) in [("small.mp4", "640x360"), ("large.mp4", "1280x720")] {
                ensure!(
                    Command::new("ffmpeg")
                        .args([
                            "-hide_banner",
                            "-loglevel",
                            "error",
                            "-y",
                            "-f",
                            "lavfi",
                            "-i"
                        ])
                        .arg(format!("testsrc2=size={size}:rate=25"))
                        .args([
                            "-f",
                            "lavfi",
                            "-i",
                            "sine=sample_rate=48000",
                            "-t",
                            "2",
                            "-c:v",
                            "libx264",
                            "-preset",
                            "ultrafast",
                            "-g",
                            "25",
                            "-c:a",
                            "aac"
                        ])
                        .arg(directory.join(name))
                        .status()?
                        .success(),
                    "creating resize fixture failed"
                );
            }
            fs::write(
                directory.join("resize.txt"),
                "file 'small.mp4'\nfile 'large.mp4'\n",
            )?;
        }
        let concat = directory.join("resize.txt");
        let done = Arc::new(AtomicBool::new(false));
        let publisher_done = done.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let publisher_cancel = cancel.clone();
        let publisher_url = url.clone();
        let log = fs::File::create(
            directory.join(format!("publisher-{full_range}-{clock_jump}-{resize}.log")),
        )?;
        let publisher = thread::spawn(move || -> Result<()> {
            let mut command = Command::new("ffmpeg");
            if resize {
                command
                    .args([
                        "-hide_banner",
                        "-loglevel",
                        "warning",
                        "-re",
                        "-readrate_catchup",
                        "4",
                        "-stream_loop",
                        "-1",
                        "-f",
                        "concat",
                        "-safe",
                        "0",
                        "-i",
                    ])
                    .arg(concat)
                    .args(["-c", "copy", "-f", "flv"]);
            } else {
                let (video_source, audio_source) = if sync_markers {
                    sync_marker_sources()
                } else {
                    (
                        "testsrc2=size=1280x720:rate=30".into(),
                        "sine=frequency=440:sample_rate=48000".into(),
                    )
                };
                command.args([
                    "-hide_banner",
                    "-loglevel",
                    "warning",
                    "-re",
                    "-readrate_catchup",
                    "4",
                    "-f",
                    "lavfi",
                    "-i",
                    &video_source,
                    "-re",
                    "-readrate_catchup",
                    "4",
                    "-f",
                    "lavfi",
                    "-i",
                    &audio_source,
                    "-c:v",
                    "libx264",
                    "-preset",
                    "veryfast",
                    "-g",
                    "30",
                    "-bf",
                    "2",
                    "-pix_fmt",
                    "yuv420p",
                    "-color_range",
                    if full_range { "pc" } else { "tv" },
                    "-vf",
                    if clock_jump {
                        "setpts=PTS+1.96/TB*gte(T\\,3)"
                    } else if sync_markers {
                        "select='not(between(t,6,7.8))'"
                    } else {
                        "null"
                    },
                    "-fps_mode",
                    "passthrough",
                    "-c:a",
                    "aac",
                    "-f",
                    "flv",
                ]);
            }
            let child = command
                .arg(publisher_url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log)
                .spawn()?;
            let publisher = Publisher(child);
            for pause in [2, 3] {
                if !publisher_wait(Duration::from_secs(5), &publisher_cancel) {
                    return Ok(());
                }
                publisher.signal("-STOP")?;
                if !publisher_wait(Duration::from_secs(pause), &publisher_cancel) {
                    return Ok(());
                }
                publisher.signal("-CONT")?;
            }
            publisher_wait(Duration::from_secs(5), &publisher_cancel);
            // Announce the intentional disconnect before the receiver can
            // observe EOF caused by killing the publisher.
            publisher_done.store(true, Ordering::Release);
            drop(publisher);
            Ok(())
        });
        let deadline = Instant::now() + Duration::from_secs(45);
        let previous_session = live.session_id;
        let mut accepted = false;
        let reception = (|| -> Result<()> {
            while Instant::now() < deadline {
                LiveOverrideOutput::new(&mut program_output, &mut live, &control).pump_live()?;
                accepted |= live.active && live.session_id > previous_session;
                ensure!(
                    !status
                        .snapshot()
                        .values()
                        .any(|state| state.phase == "failed"),
                    "HLS failed: {:?}",
                    status.snapshot()
                );
                ensure!(
                    !accepted || live.active || done.load(Ordering::Acquire),
                    "ingest ended while the publisher was still running"
                );
                if (done.load(Ordering::Acquire) || publisher.is_finished()) && !live.active {
                    break;
                }
                thread::sleep(Duration::from_millis(5));
            }
            Ok(())
        })();
        cancel.store(true, Ordering::Release);
        let publishing = publisher.join().expect("publisher thread panicked");
        reception?;
        publishing?;
        ensure!(
            accepted,
            "RTMP input never reached playout (full_range={full_range})"
        );
        ensure!(!live.active, "live input did not return to file playback");
        completed_cases += 1;
        eprintln!(
            "RTMP recovery passed: full_range={full_range}, clock_jump={clock_jump}, resize={resize}, session={}",
            live.session_id
        );
    }
    drop(live);
    program_output.finish()?;
    drop(program_output);
    output.finish()?;
    ensure!(completed_cases > 0, "no RTMP test case selected");
    for index in 0..3 {
        let path = directory.join(format!("v{index}.m3u8"));
        let playlist = fs::read_to_string(&path)?;
        ensure!(
            playlist.contains("#EXTINF"),
            "{}: no segments",
            path.display()
        );
        // The production playlist stays resumable. Seal only this finished
        // test artifact so readers stop at EOF rather than waiting for updates.
        fs::write(&path, format!("{playlist}\n#EXT-X-ENDLIST\n"))?;
        verify_media(&path, completed_cases * 250, sync_markers)?;
    }
    verify_media(
        &directory.join(if server_profile {
            "shared.flv"
        } else {
            "independent.ts"
        }),
        completed_cases * 250,
        sync_markers,
    )?;
    if server_profile {
        let tracks = native.0.lock().unwrap();
        for index in [0, 1] {
            ensure!(
                tracks
                    .get(&index)
                    .is_some_and(|(count, _)| *count >= completed_cases * 250),
                "native output missing media: {tracks:?}"
            );
        }
    }

    Ok(())
}
