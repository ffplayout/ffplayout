use std::{
    fs,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, MutexGuard, PoisonError, atomic::Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ffmpeg_next::{Packet, Rescale, codec, format, media, rescale};

mod native;

use crate::{
    LiveIdleTimeout, LiveIngestControl, LiveInputBackend, LiveListenerConfig, OutputConfig,
    StreamType,
};

use super::{
    super::parallel::ParallelOutput, ForwardMode, LIMITS, Mailbox, OutputDestination, OutputStatus,
    OutputTarget, Retry, Routes, Session, plan_tracks,
};

fn native_lock() -> MutexGuard<'static, ()> {
    static NATIVE: Mutex<()> = Mutex::new(());

    NATIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn destination(id: i32, address: String, mode: ForwardMode) -> OutputDestination {
    let mut config = OutputConfig::new(160, 90, 25, 48_000);
    config.forward_listener = Some(1);
    config.forward_mode = mode;
    config.stream_type = StreamType::Rtmp;
    config.video_codec = "libx264".into();
    config
        .video_options
        .insert("preset".into(), "ultrafast".into());

    OutputDestination {
        id,
        config,
        target: OutputTarget::Stream { url: address },
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ffplayout-forward-{id}"));

        fs::create_dir(&path).unwrap();

        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Process(Child);

impl Process {
    fn wait(&mut self) {
        self.wait_for(Duration::from_secs(10));
    }

    fn wait_for(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;

        loop {
            if self.0.try_wait().unwrap().is_some() {
                break;
            }

            assert!(Instant::now() < deadline, "FFmpeg did not finish");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn ffmpeg() -> Command {
    let mut command = Command::new("ffmpeg");

    command.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]);
    command.stdout(Stdio::null());

    command
}

fn capture(address: &str, path: &Path) -> Process {
    Process(
        ffmpeg()
            .args([
                "-listen", "1", "-i", address, "-map", "0", "-c", "copy", "-f", "flv",
            ])
            .arg(path)
            .spawn()
            .unwrap(),
    )
}

fn assert_decodable(path: &Path) {
    let mut output = format::input(path).unwrap();
    let mut starts = [None, None];

    for (stream, packet) in output.packets() {
        let dts = packet.dts().unwrap();
        let index = stream.index();

        if starts[index].is_none() && stream.parameters().medium() == media::Type::Video {
            assert!(packet.is_key());
        }

        starts[index].get_or_insert(dts.rescale(stream.time_base(), rescale::TIME_BASE));
    }

    let [Some(first), Some(second)] = starts else {
        panic!("missing audio or video in {}", path.display());
    };

    assert!(first.abs_diff(second) < 250_000, "{starts:?}");
    assert!(
        ffmpeg()
            .args(["-xerror", "-i"])
            .arg(path)
            .args(["-f", "null", "-"])
            .status()
            .unwrap()
            .success()
    );
}

fn wait_phase(status: &OutputStatus, id: i32, phase: &str) {
    let deadline = Instant::now() + Duration::from_secs(8);

    loop {
        let snapshot = status.snapshot();

        if snapshot.get(&id).is_some_and(|state| state.phase == phase) {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "output {id} wanted {phase}: {snapshot:?}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn forwarding_overflow_discards_only_the_affected_mailbox() {
    let slow = Mailbox::new();
    let healthy = Mailbox::new();
    let packet = Packet::new(16);

    for _ in 0..=LIMITS.items {
        slow.send(&packet);
        healthy.send(&packet);

        assert!(healthy.queue.try_recv().unwrap().is_some());
    }

    assert!(slow.queue.try_recv().unwrap().is_none());
    assert!(slow.overflow.load(Ordering::Acquire));
    assert!(!healthy.overflow.load(Ordering::Acquire));
}

#[test]
fn forwarding_accepts_media_again_when_a_slow_retry_is_due() {
    let mailbox = Mailbox::new();
    let packet = Packet::new(16);
    let started = Instant::now();
    let expired = started + Duration::from_secs(180);
    let mut retry = Retry::default();

    retry.failed(started);
    retry.failed(expired);
    mailbox.update_retry_pause(&retry, expired, false);
    mailbox.send(&packet);

    assert!(mailbox.queue.try_recv().unwrap().is_none());

    let due = retry.deadline(expired).unwrap();

    mailbox.update_retry_pause(&retry, due, false);
    mailbox.send(&packet);

    assert!(mailbox.queue.try_recv().unwrap().is_some());
}

#[test]
fn forwarding_queue_bounds_bytes_before_packet_count() {
    let mailbox = Mailbox::new();
    let packet = Packet::new(LIMITS.bytes / 2 + 1);

    mailbox.send(&packet);
    assert!(!mailbox.overflow.load(Ordering::Acquire));

    mailbox.send(&packet);
    assert!(mailbox.overflow.load(Ordering::Acquire));
    assert!(mailbox.queue.try_recv().unwrap().is_none());
}

#[test]
fn forwarding_routes_update_without_touching_unchanged_destinations() {
    ffmpeg_next::init().unwrap();
    let mut config = OutputConfig::new(160, 90, 25, 48_000);
    let first = destination(4, "rtmp://127.0.0.1:1936/live/a".into(), ForwardMode::Copy);
    let second = destination(5, "rtmp://127.0.0.1:1936/live/b".into(), ForwardMode::Copy);
    config.destinations = vec![first.clone(), second.clone()];
    let _output = ParallelOutput::start(&config, false).unwrap();
    let old = config.output_status.forwarding(1);
    let mut changed = first;

    changed.target = OutputTarget::Stream {
        url: "rtmp://127.0.0.1:1936/live/changed".into(),
    };
    config
        .output_status
        .update_outputs(vec![changed, second.clone()])
        .unwrap();

    let current = config.output_status.forwarding(1);

    assert!(old[0].stopped.load(Ordering::Acquire));
    assert!(std::sync::Arc::ptr_eq(&old[1], &current[1]));
    assert_eq!(config.output_status.snapshot()[&4].phase, "waiting_input");

    config.output_status.update_outputs(vec![second]).unwrap();

    assert_eq!(config.output_status.snapshot()[&4].phase, "stopped");
}

#[test]
fn publisher_disconnect_clears_pending_forwarding_status_without_overwriting_a_new_session() {
    let mut config = OutputConfig::new(160, 90, 25, 48_000);

    config.destinations.push(destination(
        1,
        "rtmp://127.0.0.1/live/test".into(),
        ForwardMode::Copy,
    ));

    let _output = ParallelOutput::start(&config, false).unwrap();
    let status = &config.output_status;
    let route = status.forwarding(1).pop().unwrap();
    let mut old = Session::new(1, status);
    let mut new = Session::new(1, status);

    assert!(old.has_unseen_route());

    old.set_pending(&route, "waiting_keyframe", None);

    assert!(!old.has_unseen_route());
    assert!(!old.active());

    new.set_pending(&route, "failed", Some("unsupported codec".into()));
    drop(old);

    assert_eq!(status.snapshot()[&1].phase, "failed");
    assert!(!new.active());

    drop(new);

    assert_eq!(status.snapshot()[&1].phase, "waiting_input");
    assert!(status.snapshot()[&1].error.is_none());
}

#[test]
fn automatic_forwarding_decides_video_and_audio_independently() {
    ffmpeg_next::init().unwrap();
    let route = Routes::prepare(vec![destination(
        1,
        "rtmp://127.0.0.1/live/a".into(),
        ForwardMode::Auto,
    )])
    .unwrap()
    .pop()
    .unwrap();
    let track = |video: bool, id: codec::Id| {
        let mut parameters = codec::Parameters::new();

        // SAFETY: the test exclusively owns these synthetic parameters.
        unsafe {
            let raw = &mut *parameters.as_mut_ptr();
            raw.codec_id = id.into();
            raw.width = 160;
            raw.height = 90;
            raw.sample_rate = 48_000;
            raw.ch_layout.nb_channels = 2;
        }

        super::Track {
            input: usize::from(!video),
            video,
            parameters,
            time_base: ffmpeg_next::Rational(1, 90_000),
        }
    };
    let plan = plan_tracks(
        &route,
        &[track(true, codec::Id::H264), track(false, codec::Id::MP2)],
    )
    .unwrap();

    assert!(plan[0].is_none());
    assert!(plan[1].as_ref().unwrap().contains("audio"));
}

#[test]
#[ignore = "requires FFmpeg CLI and local SRT/RTMP sockets"]
fn standby_srt_forwarding_changes_destinations_without_restarting_ingest() {
    let _native = native_lock();

    ffmpeg_next::init().unwrap();
    let scratch = Scratch::new();
    let duration = std::env::var("FFPLAYOUT_FORWARD_TEST_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(12)
        .max(12);
    let input_port = port();
    let input = format!("srt://127.0.0.1:{input_port}");
    let caller = format!("{input}?mode=caller");
    let a = format!("rtmp://127.0.0.1:{}/live/a", port());
    let b = format!("rtmp://127.0.0.1:{}/live/b", port());
    let first_file = scratch.0.join("first.flv");
    let second_file = scratch.0.join("second.flv");
    let replacement_file = scratch.0.join("replacement.flv");
    let mut first_receiver = capture(&a, &first_file);
    let mut second_receiver = capture(&b, &second_file);
    let first = destination(4, a, ForwardMode::Copy);
    let second = destination(5, b, ForwardMode::Copy);
    let config = OutputConfig::new(160, 90, 25, 48_000);
    let control = LiveIngestControl::default();

    control.set_automatic(1, false);
    let output = ParallelOutput::start(&config, false).unwrap();
    let _live = crate::input::live::spawn_controlled_live_listeners(
        vec![LiveListenerConfig {
            id: 1,
            priority: 0,
            idle_timeout: LiveIdleTimeout::default(),
            backend: LiveInputBackend::Srt,
            url: input,
            options: Default::default(),
            demuxer_options: [("analyzeduration".into(), "250000".into())].into(),
        }],
        config.clone(),
        control.clone(),
    );

    thread::sleep(Duration::from_millis(300));
    let mut publisher = Process(
        ffmpeg()
            .args([
                "-re",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x90:rate=25",
                "-f",
                "lavfi",
                "-i",
                "sine=sample_rate=48000",
                "-t",
                &duration.to_string(),
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-g",
                "25",
                "-c:a",
                "aac",
                "-ac",
                "2",
                "-f",
                "mpegts",
                &caller,
            ])
            .spawn()
            .unwrap(),
    );

    let deadline = Instant::now() + Duration::from_secs(8);
    let initial = loop {
        if let Some(input) = control.snapshot().into_iter().find(|input| input.ready) {
            break input;
        }

        assert!(Instant::now() < deadline, "standby input did not connect");
        thread::sleep(Duration::from_millis(20));
    };

    assert!(config.output_status.forwarding(1).is_empty());
    assert!(!initial.selected);

    config
        .output_status
        .update_outputs(vec![first.clone()])
        .unwrap();
    wait_phase(&config.output_status, 4, "copying");

    config
        .output_status
        .update_outputs(vec![first, second.clone()])
        .unwrap();
    wait_phase(&config.output_status, 5, "copying");
    let unreachable = destination(
        6,
        format!("rtmp://127.0.0.1:{}/live/unreachable", port()),
        ForwardMode::Copy,
    );

    config
        .output_status
        .update_outputs(vec![second.clone(), unreachable])
        .unwrap();
    wait_phase(&config.output_status, 4, "stopped");
    wait_phase(&config.output_status, 6, "retrying");

    // With no consumer draining the selected input's program queue, the
    // upstream publisher must still reach the forwarding destination.
    control.select(1, &initial.session, true).unwrap();
    thread::sleep(Duration::from_secs(2));
    control.select(1, &initial.session, false).unwrap();
    let replacement_url = format!("rtmp://127.0.0.1:{}/live/replacement", port());
    let mut replacement_receiver = capture(&replacement_url, &replacement_file);
    let replacement = destination(4, replacement_url, ForwardMode::Copy);

    config
        .output_status
        .update_outputs(vec![second, replacement])
        .unwrap();
    wait_phase(&config.output_status, 4, "copying");

    let current = control
        .snapshot()
        .into_iter()
        .find(|input| input.id == 1)
        .unwrap();

    assert_eq!(initial.session, current.session);
    assert!(!current.selected);
    assert_eq!(config.output_status.snapshot()[&5].phase, "copying");

    publisher.wait_for(Duration::from_secs(duration.saturating_add(5)));
    first_receiver.wait();
    second_receiver.wait();
    replacement_receiver.wait();
    assert_decodable(&first_file);
    assert_decodable(&second_file);
    assert_decodable(&replacement_file);
    drop(output);
}

#[test]
#[ignore = "requires FFmpeg CLI and a local RTMP socket"]
fn forwarding_transcodes_only_the_selected_tracks() {
    let _native = native_lock();

    ffmpeg_next::init().unwrap();
    let scratch = Scratch::new();

    for (video, audio, mode) in [
        ("libx264", "mp2", ForwardMode::Auto),
        ("mpeg2video", "aac", ForwardMode::Auto),
        ("libx264", "aac", ForwardMode::Transcode),
    ] {
        let source_file = scratch.0.join("source.ts");
        let output_file = scratch.0.join("capture.flv");
        assert!(
            ffmpeg()
                .args([
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=160x90:rate=25",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=sample_rate=48000",
                    "-c:v",
                    video,
                    "-g",
                    "25",
                    "-c:a",
                    audio,
                    "-ac",
                    "2",
                    "-t",
                    "3",
                    "-f",
                    "mpegts",
                ])
                .arg(&source_file)
                .status()
                .unwrap()
                .success()
        );
        let address = format!("rtmp://127.0.0.1:{}/live/test", port());
        let mut receiver = capture(&address, &output_file);
        let mut config = OutputConfig::new(160, 90, 25, 48_000);
        config.destinations.push(destination(4, address, mode));
        let shared_file = scratch.0.join("shared.flv");
        let shared_receiver = (mode == ForwardMode::Transcode).then(|| {
            let address = format!("rtmp://127.0.0.1:{}/live/shared", port());
            let receiver = capture(&address, &shared_file);

            config.destinations.push(destination(5, address, mode));

            receiver
        });
        let runtime = ParallelOutput::start(&config, false).unwrap();
        let mut session = Session::new(1, &config.output_status);
        let mut input = format::input(&source_file).unwrap();

        thread::sleep(Duration::from_millis(150));
        loop {
            let mut packet = Packet::empty();

            if packet.read(&mut input).is_err() {
                break;
            }

            session.publish(&input, &packet);
            thread::sleep(Duration::from_millis(6));
        }

        wait_phase(&config.output_status, 4, "transcoding");

        if shared_receiver.is_some() {
            wait_phase(&config.output_status, 5, "transcoding");
        }

        assert_eq!(session.groups.len(), 1);
        drop(session);
        receiver.wait();
        assert_decodable(&output_file);

        if let Some(mut receiver) = shared_receiver {
            receiver.wait();
            assert_decodable(&shared_file);
        }

        drop(runtime);
    }
}
