//! Manual end-to-end recovery test: FFmpeg publisher -> RTMP decoder -> HLS.
use std::{
    collections::BTreeMap,
    fs,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};

use super::LiveOverrideOutput;
use crate::{
    HlsVariant, OutputConfig, PlaybackControl,
    input::live::{LiveIdleTimeout, LiveInputBackend, LiveListenerConfig, spawn_live_listeners},
    output::{
        Output,
        parallel::{OutputDestination, OutputTarget},
    },
};

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

fn verify_media(path: &Path, minimum_packets: usize) -> Result<()> {
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
    let decoded = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-xerror",
            "-err_detect",
            "explode",
            "-i",
        ])
        .arg(path)
        .args(["-map", "0:v:0", "-map", "0:a:0", "-f", "null", "-"])
        .output()?;
    ensure!(
        decoded.status.success(),
        "{}: decoding failed: {}",
        path.display(),
        String::from_utf8_lossy(&decoded.stderr)
    );

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
    config.destinations.push(OutputDestination {
        id: 5,
        config: config.clone(),
        target: OutputTarget::Hls {
            playlist: directory.join("master.m3u8").to_string_lossy().into_owned(),
            variants: [(1280, 720), (854, 480), (480, 270)]
                .into_iter()
                .enumerate()
                .map(|(i, (width, height))| HlsVariant {
                    name: format!("v{i}"),
                    width,
                    height,
                    video_bitrate: 2_000_000,
                    audio_bitrate: 128_000,
                })
                .collect(),
            subtitle: None,
            segment_seconds: 1,
            list_size: 120,
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
    config.destinations.push(OutputDestination {
        id: 6,
        config: independent,
        target: OutputTarget::Stream {
            url: directory
                .join("independent.ts")
                .to_string_lossy()
                .into_owned(),
        },
    });
    let status = config.output_status.clone();
    let mut output = Output::open_parallel(&config)?;
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
                    "testsrc2=size=1280x720:rate=30",
                    "-re",
                    "-readrate_catchup",
                    "4",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000",
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
                LiveOverrideOutput::new(&mut output, &mut live, &control).pump_live()?;
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
        verify_media(&path, completed_cases * 250)?;
    }
    verify_media(&directory.join("independent.ts"), completed_cases * 250)?;

    Ok(())
}
