use std::{process::Command, thread, time::Duration};

#[cfg(target_os = "linux")]
use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::JoinHandle,
    time::Instant,
};

use ffmpeg_next::{Packet, codec, format, media};
#[cfg(target_os = "linux")]
use ffmpeg_next::{Rescale, rescale};

#[cfg(target_os = "linux")]
use crate::{ClipResult, Playout, StreamType};
use crate::{
    LiveIdleTimeout, LiveIngestControl, LiveInputBackend, LiveListenerConfig, OutputConfig,
};

use super::{
    super::source::Source, ForwardMode, ParallelOutput, Process, Scratch, Session,
    assert_decodable, capture, destination, ffmpeg, native_lock, port, wait_phase,
};
#[cfg(target_os = "linux")]
use super::{OutputDestination, OutputTarget};

fn generate_av(target: &str, dimensions: &str, seconds: u64, video: &str, gop: u32) -> Command {
    let mut command = ffmpeg();

    if target.starts_with("srt://") {
        command.arg("-re");
    }

    command.args([
        "-f",
        "lavfi",
        "-i",
        &format!("testsrc2=size={dimensions}:rate=25"),
        "-f",
        "lavfi",
        "-i",
        "sine=sample_rate=48000",
        "-t",
        &seconds.to_string(),
        "-c:v",
        video,
        "-threads:v",
        "2",
        "-g",
        &gop.to_string(),
        "-sc_threshold",
        "0",
        "-c:a",
        "aac",
        "-ac",
        "2",
        "-f",
        "mpegts",
    ]);

    if video == "libx264" {
        command.args(["-preset", "ultrafast", "-tune", "zerolatency"]);
    }

    command.arg(target);

    command
}

fn srt_listener(port: u16) -> LiveListenerConfig {
    LiveListenerConfig {
        id: 1,
        priority: 0,
        idle_timeout: LiveIdleTimeout::default(),
        backend: LiveInputBackend::Srt,
        url: format!("srt://127.0.0.1:{port}"),
        options: Default::default(),
        demuxer_options: [("analyzeduration".into(), "250000".into())].into(),
    }
}

#[cfg(target_os = "linux")]
fn wait_until(timeout: Duration, description: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;

    while !ready() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "linux")]
struct Playlist {
    stop: Arc<AtomicBool>,
    clips: Arc<AtomicUsize>,
    control: LiveIngestControl,
    worker: Option<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Playlist {
    fn start(config: OutputConfig, input: LiveListenerConfig, clip: PathBuf) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let clips = Arc::new(AtomicUsize::new(0));
        let (ready, control) = std::sync::mpsc::sync_channel(1);
        let worker = thread::spawn({
            let stop = stop.clone();
            let clips = clips.clone();

            move || {
                let mut playout = Playout::open_parallel(config.clone(), 0.1).unwrap();
                let control = playout.playback_control.live_ingest();

                control.set_automatic(input.id, false);

                let mut live = Some(crate::input::live::spawn_controlled_live_listeners(
                    vec![input],
                    config,
                    control.clone(),
                ));

                ready.send(control).unwrap();

                while !stop.load(Ordering::Acquire) {
                    assert_eq!(
                        playout
                            .play_with_live(clip.to_str().unwrap(), None, &mut live)
                            .unwrap(),
                        ClipResult::Played
                    );
                    clips.fetch_add(1, Ordering::Release);
                }

                drop(live);
                playout.finish().unwrap();
            }
        });

        Self {
            stop,
            clips,
            control: control.recv().unwrap(),
            worker: Some(worker),
        }
    }

    fn finish(mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[cfg(target_os = "linux")]
impl Drop for Playlist {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);

        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(target_os = "linux")]
fn forwarding_threads() -> (usize, usize) {
    fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(|task| fs::read_to_string(task.ok()?.path().join("comm")).ok())
        .fold((0, 0), |(delivery, transcode), name| {
            (
                delivery + usize::from(name.starts_with("ingest-forward")),
                transcode + usize::from(name.starts_with("ingest-transcod")),
            )
        })
}

#[cfg(target_os = "linux")]
fn wait_for_rtmp_listener(address: &str) {
    let port: u16 = address
        .split('/')
        .nth(2)
        .unwrap()
        .rsplit(':')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let local = format!("0100007F:{port:04X}");

    wait_until(Duration::from_secs(5), address, || {
        fs::read_to_string("/proc/net/tcp")
            .unwrap()
            .lines()
            .any(|entry| {
                let fields: Vec<_> = entry.split_ascii_whitespace().take(4).collect();

                fields.get(1) == Some(&local.as_str()) && fields.get(3) == Some(&"0A")
            })
    });
}

#[cfg(target_os = "linux")]
fn accept_slow_connection(listener: &TcpListener) -> TcpStream {
    let mut accepted = None;

    wait_until(
        Duration::from_secs(20),
        "RTMP connection",
        || match listener.accept() {
            Ok((stream, _)) => {
                accepted = Some(stream);

                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
            Err(error) => panic!("accept failed: {error}"),
        },
    );

    accepted.unwrap()
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires Linux, FFmpeg CLI and local SRT/RTMP sockets"]
fn forwarding_during_playlist_survives_slow_destination_and_releases_workers() {
    let _native = native_lock();

    let scratch = Scratch::new();
    let seconds = std::env::var("FFPLAYOUT_FORWARD_TEST_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30)
        .max(30);
    let exhaust_retry = seconds >= 230;
    let dimensions =
        std::env::var("FFPLAYOUT_FORWARD_TEST_SIZE").unwrap_or_else(|_| "160x90".into());
    let (width, height) = dimensions.split_once('x').unwrap();
    let ingress = srt_listener(port());
    let caller = format!("{}?mode=caller", ingress.url);
    let clip = scratch.0.join("clip.ts");
    let program_file = scratch.0.join("program.ts");
    let mut config = OutputConfig::new(width.parse().unwrap(), height.parse().unwrap(), 25, 48_000);
    let mut program_cfg = OutputDestination {
        id: 1,
        config: config.clone(),
        target: OutputTarget::Stream {
            url: program_file.to_str().unwrap().into(),
        },
    };

    assert!(
        generate_av(clip.to_str().unwrap(), &dimensions, 2, "libx264", 25)
            .status()
            .unwrap()
            .success()
    );

    program_cfg.config.stream_type = StreamType::Custom;
    program_cfg.config.stream_format = "mpegts".into();
    program_cfg
        .config
        .video_options
        .insert("preset".into(), "ultrafast".into());
    program_cfg
        .config
        .video_options
        .insert("tune".into(), "zerolatency".into());
    config.destinations = vec![program_cfg.clone()];

    let playlist = Playlist::start(config.clone(), ingress, clip);
    let status = &config.output_status;

    wait_until(Duration::from_secs(6), "playlist output", || {
        playlist.clips.load(Ordering::Acquire) >= 1
    });

    let mut publisher = Process(
        generate_av(&caller, &dimensions, seconds, "libx264", 25)
            .spawn()
            .unwrap(),
    );

    wait_until(Duration::from_secs(8), "standby publisher", || {
        playlist.control.snapshot().iter().any(|input| input.ready)
    });

    let original = playlist.control.snapshot().pop().unwrap();
    let slow = TcpListener::bind("127.0.0.1:0").unwrap();
    let slow_address = format!("rtmp://{}/live/slow", slow.local_addr().unwrap());
    let slow_target = destination(6, slow_address.clone(), ForwardMode::Copy);

    slow.set_nonblocking(true).unwrap();
    assert_eq!(forwarding_threads(), (0, 0));

    let disabled = |mut target: OutputDestination| {
        target.config.delivery_enabled = false;

        target
    };

    for cycle in 0..2 {
        let a = format!("rtmp://127.0.0.1:{}/live/a", port());
        let b = format!("rtmp://127.0.0.1:{}/live/b", port());
        let file_a = scratch.0.join(format!("a-{cycle}.flv"));
        let file_b = scratch.0.join(format!("b-{cycle}.flv"));
        let mut receiver_a = capture(&a, &file_a);
        let mut receiver_b = capture(&b, &file_b);

        wait_for_rtmp_listener(&a);
        wait_for_rtmp_listener(&b);

        let a = destination(4, a, ForwardMode::Transcode);
        let b = destination(5, b, ForwardMode::Transcode);

        status
            .update_outputs(vec![program_cfg.clone(), a.clone(), b.clone()])
            .unwrap();
        wait_phase(status, 4, "transcoding");
        wait_phase(status, 5, "transcoding");
        let (deliveries, encoding_threads) = forwarding_threads();

        assert_eq!(deliveries, 2);
        assert!(encoding_threads > 0);

        let clips = playlist.clips.load(Ordering::Acquire);

        wait_until(Duration::from_secs(8), "another playlist clip", || {
            playlist.clips.load(Ordering::Acquire) > clips
        });
        status
            .update_outputs(vec![program_cfg.clone(), disabled(a), disabled(b)])
            .unwrap();
        wait_until(Duration::from_secs(5), "forwarding workers to exit", || {
            forwarding_threads() == (0, 0)
        });
        receiver_a.wait();
        receiver_b.wait();
        assert_decodable(&file_a);
        assert_decodable(&file_b);
        assert_eq!(playlist.control.snapshot()[0].session, original.session);
        assert!(!playlist.control.snapshot()[0].selected);
    }

    let addr = format!("rtmp://127.0.0.1:{}/live/healthy", port());
    let healthy_file = scratch.0.join("healthy.flv");
    let mut healthy_receiver = capture(&addr, &healthy_file);

    wait_for_rtmp_listener(&addr);

    let healthy = destination(4, addr, ForwardMode::Copy);

    status
        .update_outputs(vec![program_cfg, healthy, slow_target])
        .unwrap();
    wait_phase(status, 4, "copying");

    let stalled = accept_slow_connection(&slow);
    let clips = playlist.clips.load(Ordering::Acquire);

    wait_until(
        Duration::from_secs(7),
        "playlist progress with a stalled RTMP peer",
        || playlist.clips.load(Ordering::Acquire) >= clips + 2,
    );
    assert_eq!(status.snapshot()[&4].phase, "copying");

    drop(stalled);
    wait_phase(status, 6, "retrying");

    if exhaust_retry {
        let second_attempt = accept_slow_connection(&slow);

        drop(second_attempt);
        drop(slow);
        wait_until(Duration::from_secs(185), "retry exhaustion", || {
            status.can_retry(6)
        });

        let recovered_file = scratch.0.join("recovered.flv");
        let mut recovered = capture(&slow_address, &recovered_file);

        wait_for_rtmp_listener(&slow_address);
        assert!(status.request_retry(6));
        wait_phase(status, 6, "copying");
        assert_eq!(status.snapshot()[&4].phase, "copying");
        assert_eq!(playlist.control.snapshot()[0].session, original.session);

        publisher.wait_for(Duration::from_secs(seconds + 5));
        recovered.wait();
        assert_decodable(&recovered_file);
    } else {
        publisher.wait_for(Duration::from_secs(seconds + 5));
    }

    healthy_receiver.wait();
    assert_decodable(&healthy_file);
    assert_continuous_av(&healthy_file, seconds - 15);
    playlist.finish();
    wait_until(Duration::from_secs(5), "forwarding shutdown", || {
        forwarding_threads() == (0, 0)
    });
    assert_continuous_av(&program_file, seconds);
}

#[cfg(target_os = "linux")]
fn assert_continuous_av(path: &Path, seconds: u64) {
    let mut input = format::input(path).unwrap();
    let mut previous = [None, None];
    let mut frames = [0_u64; 2];

    for (stream, packet) in input.packets() {
        let index = stream.index();
        let pts = packet
            .dts()
            .unwrap()
            .rescale(stream.time_base(), rescale::TIME_BASE);

        if let Some(last) = previous[index] {
            assert!(
                (1..100_000).contains(&(pts - last)),
                "stream {index}: {last} -> {pts}"
            );
        }

        previous[index] = Some(pts);
        frames[index] += 1;

        if let [Some(first), Some(second)] = previous {
            assert!(
                first.abs_diff(second) < 250_000,
                "A/V timestamps {previous:?}"
            );
        }
    }

    assert!(frames[0] >= seconds * 24, "{frames:?}");
    assert!(frames[1] >= seconds * 45, "{frames:?}");
}

#[test]
#[ignore = "requires FFmpeg CLI and local SRT/RTMP sockets"]
fn forwarding_reprobes_media_after_the_srt_publisher_reconnects() {
    let _native = native_lock();

    ffmpeg_next::init().unwrap();
    let scratch = Scratch::new();
    let ingress = srt_listener(port());
    let caller = format!("{}?mode=caller", ingress.url);
    let address = format!("rtmp://127.0.0.1:{}/live/reconnect", port());
    let mut config = OutputConfig::new(160, 90, 25, 48_000);
    let control = LiveIngestControl::default();

    control.set_automatic(1, false);
    config
        .destinations
        .push(destination(4, address.clone(), ForwardMode::Auto));

    let runtime = ParallelOutput::start(&config, false).unwrap();
    let live = crate::input::live::spawn_controlled_live_listeners(
        vec![ingress],
        config.clone(),
        control.clone(),
    );
    let mut previous = None;

    for (size, video, expected) in [
        ((160, 90), "libx264", "copying"),
        ((320, 180), "mpeg2video", "transcoding"),
        ((0, 0), "", "copying"),
    ] {
        let file = scratch.0.join(format!("reconnect-{}.flv", size.0));
        let mut receiver = capture(&address, &file);
        let mut command = if video.is_empty() {
            let mut command = ffmpeg();

            command.args([
                "-re",
                "-f",
                "lavfi",
                "-i",
                "sine=sample_rate=48000",
                "-t",
                "4",
                "-c:a",
                "aac",
                "-ac",
                "2",
                "-f",
                "mpegts",
                &caller,
            ]);

            command
        } else {
            generate_av(&caller, &format!("{}x{}", size.0, size.1), 4, video, 25)
        };

        thread::sleep(Duration::from_millis(400));

        let mut publisher = Process(command.spawn().unwrap());

        wait_phase(&config.output_status, 4, expected);

        let current = control.snapshot().pop().unwrap();

        assert_ne!(previous.as_ref(), Some(&current.session));
        assert!(!current.selected);

        previous = Some(current.session);

        publisher.wait();
        wait_phase(&config.output_status, 4, "waiting_input");
        receiver.wait();

        let recorded = format::input(&file).unwrap();

        if video.is_empty() {
            assert!(recorded.streams().best(media::Type::Video).is_none());
            assert_eq!(
                recorded
                    .streams()
                    .best(media::Type::Audio)
                    .unwrap()
                    .parameters()
                    .id(),
                codec::Id::AAC
            );
        } else {
            let parameters = recorded
                .streams()
                .best(media::Type::Video)
                .unwrap()
                .parameters();
            let decoder = codec::context::Context::from_parameters(parameters)
                .unwrap()
                .decoder()
                .video()
                .unwrap();

            assert_eq!((decoder.width(), decoder.height()), size);
            assert_eq!(decoder.id(), codec::Id::H264);
            assert_decodable(&file);
        }
    }

    drop(live);
    drop(runtime);
}

#[test]
#[ignore = "requires FFmpeg CLI to generate a long GOP fixture"]
fn forwarding_restarts_on_timestamp_and_codec_changes_and_waits_for_in_band_headers() {
    ffmpeg_next::init().unwrap();
    let scratch = Scratch::new();
    let fixture = scratch.0.join("long-gop.ts");

    assert!(
        generate_av(fixture.to_str().unwrap(), "160x90", 6, "libx264", 100)
            .status()
            .unwrap()
            .success()
    );

    let mut input = format::input(&fixture).unwrap();
    let video = input.streams().best(media::Type::Video).unwrap().index();
    let mut parameters = input.stream(video).unwrap().parameters().clone();
    let mut packets: Vec<_> = input
        .packets()
        .filter_map(|(stream, packet)| (stream.index() == video).then_some(packet))
        .collect();
    let mut source = Source::default();

    // SAFETY: the test owns this cloned AVCodecParameters. FFmpeg allocated
    // its extra data, and the demuxer receives a separate copied parameter set.
    unsafe {
        let raw = &mut *parameters.as_mut_ptr();
        ffmpeg_next::ffi::av_freep(std::ptr::from_mut(&mut raw.extradata).cast());
        raw.extradata_size = 0;
    }

    input
        .stream_mut(video)
        .unwrap()
        .set_parameters(parameters.clone());

    for packet in &packets[1..100] {
        assert!(!packet.is_key());
        source.update(&input, packet);
        assert!(!source.ready());
    }

    assert!(packets[100].is_key());
    assert!(source.update(&input, &packets[100]));
    assert!(source.ready());
    assert!(!source.update(&input, &packets[101]));

    packets[102].set_dts(Some(0));

    assert!(source.update(&input, &packets[102]));

    // SAFETY: the same privately owned parameters are copied into the demux
    // stream to simulate its codec and dimensions changing between packets.
    unsafe {
        let raw = &mut *parameters.as_mut_ptr();
        raw.codec_id = codec::Id::MPEG2VIDEO.into();
        raw.width = 320;
        raw.height = 180;
    }

    input.stream_mut(video).unwrap().set_parameters(parameters);

    assert!(source.update(&input, &packets[103]));

    let track = source.tracks().remove(0);
    let decoder = codec::context::Context::from_parameters(track.parameters)
        .unwrap()
        .decoder()
        .video()
        .unwrap();

    assert_eq!(decoder.id(), codec::Id::MPEG2VIDEO);
    assert_eq!((decoder.width(), decoder.height()), (320, 180));
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "requires Linux, FFmpeg CLI and a local RTMP socket"]
fn forwarding_bounds_queue_and_cancels_a_blocked_rtmp_write() {
    let _native = native_lock();

    ffmpeg_next::init().unwrap();
    let scratch = Scratch::new();
    let fixture = scratch.0.join("high-bitrate.ts");

    assert!(
        ffmpeg()
            .args([
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=25,noise=alls=65:allf=t",
                "-t",
                "8",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                "-threads:v",
                "2",
                "-crf",
                "8",
                "-g",
                "25",
                "-f",
                "mpegts",
            ])
            .arg(&fixture)
            .status()
            .unwrap()
            .success()
    );

    let address = format!("rtmp://127.0.0.1:{}/live/paused", port());
    let receiver = capture(&address, &scratch.0.join("paused.flv"));

    wait_for_rtmp_listener(&address);

    let mut config = OutputConfig::new(640, 360, 25, 48_000);

    config
        .destinations
        .push(destination(4, address, ForwardMode::Copy));

    let output = ParallelOutput::start(&config, false).unwrap();
    let mut session = Session::new(1, &config.output_status);
    let mut input = format::input(&fixture).unwrap();
    let mut packet = Packet::empty();

    packet.read(&mut input).unwrap();
    session.publish(&input, &packet);
    wait_phase(&config.output_status, 4, "copying");

    // SAFETY: the signal targets only the FFmpeg child owned by receiver.
    assert_eq!(
        unsafe { libc::kill(receiver.0.id() as libc::pid_t, libc::SIGSTOP) },
        0
    );

    let mailbox = session.workers[&4].mailbox.clone();
    let mut overflow = false;

    loop {
        let mut packet = Packet::empty();

        if packet.read(&mut input).is_err() {
            break;
        }

        session.publish(&input, &packet);

        if mailbox.overflow.load(Ordering::Acquire) {
            overflow = true;
            break;
        }

        thread::sleep(Duration::from_millis(2));
    }

    assert!(
        overflow,
        "the stalled receiver did not overflow its bounded queue"
    );

    // A blocked writer cannot consume the overflow flag until FFmpeg returns.
    thread::sleep(Duration::from_millis(100));
    assert!(mailbox.overflow.load(Ordering::Acquire));

    let started = Instant::now();

    config.output_status.update_outputs(Vec::new()).unwrap();
    assert!(!session.publish(&input, &Packet::empty()));
    assert!(session.workers.is_empty());
    assert!(session.source.tracks().is_empty());

    drop(session);
    drop(output);

    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(mailbox.control.cancelled());
    assert!(
        mailbox
            .queue
            .capacity_description(0, None)
            .contains("queued_items=0")
    );
    assert_eq!(forwarding_threads(), (0, 0));
}

#[test]
#[ignore = "requires FFmpeg CLI to generate the AAC fixture"]
fn forwarding_decoder_recovers_from_a_damaged_aac_packet() {
    use std::{sync::Arc, time::Instant};

    use super::super::{Mailbox, Routes, Track, group::Group};

    ffmpeg_next::init().unwrap();
    let scratch = Scratch::new();
    let fixture = scratch.0.join("decode-recovery.ts");

    assert!(
        generate_av(fixture.to_str().unwrap(), "160x90", 2, "libx264", 25)
            .status()
            .unwrap()
            .success()
    );

    let mut input = format::input(&fixture).unwrap();
    let track = Track::from_stream(input.streams().best(media::Type::Audio).unwrap(), false);
    let mut probe = codec::context::Context::from_parameters(track.parameters.clone())
        .unwrap()
        .decoder()
        .audio()
        .unwrap();
    let mut invalid = Packet::copy(&[0xff; 3]);

    assert!(
        probe
            .send_packet(&invalid)
            .and_then(|()| probe.receive_frame(&mut ffmpeg_next::frame::Audio::empty()))
            .is_err()
    );
    invalid.set_stream(track.input);

    let route = Routes::prepare(vec![destination(
        1,
        "rtmp://127.0.0.1/live/recovery".into(),
        ForwardMode::Transcode,
    )])
    .unwrap()
    .pop()
    .unwrap();
    let route = Arc::new(route);
    let consumer = Mailbox::new();
    let group = Group::start(&route, vec![track.clone()], vec![Some("audio".into())]).unwrap();

    assert!(group.try_add(&consumer, &route));
    group.send(&invalid);

    for (_, packet) in input
        .packets()
        .filter(|(stream, _)| stream.index() == track.input)
    {
        group.send(&packet);
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut encoded = 0;

    while encoded < 20 {
        assert!(
            Instant::now() < deadline,
            "forwarding did not recover after damaged AAC"
        );

        if consumer.queue.try_recv().unwrap().is_some() {
            encoded += 1;
        } else {
            thread::sleep(Duration::from_millis(5));
        }
    }

    group.stop().join().unwrap();
}

#[test]
#[ignore = "requires FFmpeg CLI and a local UDP socket"]
fn forwarding_mux_flushes_when_an_announced_track_is_missing() {
    use std::{net::UdpSocket, sync::Arc, time::Instant};

    use crate::{StreamType, utils::pipeline_queue::PipelineControl};

    use super::super::{MUX_FLUSH_INTERVAL, Mux, Routes, Stop, Track};

    ffmpeg_next::init().unwrap();
    let scratch = Scratch::new();
    let fixture = scratch.0.join("interleave.ts");

    assert!(
        generate_av(fixture.to_str().unwrap(), "160x90", 2, "libx264", 25)
            .status()
            .unwrap()
            .success()
    );

    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let url = format!("udp://{}", socket.local_addr().unwrap());
    let mut target = destination(1, url, ForwardMode::Copy);

    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    target.config.stream_type = StreamType::Udp;
    target
        .config
        .muxer_options
        .insert("max_interleave_delta".into(), "0".into());
    target
        .config
        .muxer_options
        .insert("flush_packets".into(), "1".into());

    let route = Routes::prepare(vec![target]).unwrap().pop().unwrap();
    let control = Arc::new(PipelineControl::default());
    let stop = Stop {
        route: &route,
        control: &control,
    };
    let mut input = format::input(&fixture).unwrap();
    let tracks = [
        Track::from_stream(input.streams().best(media::Type::Video).unwrap(), true),
        Track::from_stream(input.streams().best(media::Type::Audio).unwrap(), false),
    ];
    let packet = input
        .packets()
        .find(|(stream, _)| stream.index() == tracks[0].input)
        .unwrap()
        .1;
    let mut mux = Mux::open(&stop, &tracks, 0).unwrap();

    assert!(!mux.has_written_media());

    mux.write(packet, &tracks[0], 0).unwrap();
    assert_eq!(mux.pending_packets, 1);
    assert!(!mux.has_written_media());

    mux.flush_if_due(Instant::now() + MUX_FLUSH_INTERVAL, 0, 0)
        .unwrap();
    assert_eq!(mux.pending_packets, 0);
    assert_eq!(mux.pending_bytes, 0);
    assert!(mux.has_written_media());

    let mut datagram = [0_u8; 2048];

    assert!(socket.recv(&mut datagram).unwrap() >= 188);
    assert_eq!(datagram[0], 0x47);
}

#[test]
#[ignore = "requires FFmpeg CLI to generate the MPEG-2 fixture"]
fn incompatible_copy_route_does_not_activate_packet_forwarding() {
    ffmpeg_next::init().unwrap();
    let scratch = Scratch::new();
    let fixture = scratch.0.join("incompatible.ts");

    assert!(
        generate_av(fixture.to_str().unwrap(), "160x90", 2, "mpeg2video", 25)
            .status()
            .unwrap()
            .success()
    );

    let mut config = OutputConfig::new(160, 90, 25, 48_000);

    config.destinations.push(destination(
        1,
        "rtmp://127.0.0.1/live/incompatible".into(),
        ForwardMode::Copy,
    ));

    let _output = ParallelOutput::start(&config, false).unwrap();
    let status = &config.output_status;
    let mut session = Session::new(1, status);
    let mut input = format::input(&fixture).unwrap();
    let mut packet = Packet::empty();

    packet.read(&mut input).unwrap();

    assert!(!session.publish(&input, &packet));
    assert!(!session.active());
    assert!(!session.has_unseen_route());
    assert_eq!(status.snapshot()[&1].phase, "failed");

    drop(session);

    assert_eq!(status.snapshot()[&1].phase, "waiting_input");
}
