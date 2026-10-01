use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use ffmpeg_next::frame;

use super::live::{
    ListenerSlot, LiveEvent, LiveFrameSender, LiveOverrideOutput, LiveReceiver,
    live_channel_capacity, relay_live_listeners,
};
use super::playback::resume_pts;
use crate::{
    audio_mixer::{LiveLoudnessConfig, LiveLoudnessControl, LiveLoudnessProcessor},
    output::FrameOutput,
};

#[derive(Default)]
struct CountingOutput {
    video_frames: usize,
    audio_frames: usize,
    audio_pts: Vec<i64>,
    last_audio: Option<(i64, usize, f32)>,
    reset_after_skip: bool,
    skip_target: Option<(i64, i64)>,
    virtual_audio_padding: bool,
    padded_audio_samples: i64,
    interrupt: Option<(crate::PlaybackControl, bool)>,
}

impl FrameOutput for CountingOutput {
    fn audio_frame_size(&self) -> usize {
        1024
    }

    fn encode_video(&mut self, _frame: &frame::Video) -> Result<()> {
        self.video_frames += 1;

        if let Some((control, restart)) = self.interrupt.take() {
            if restart {
                control.restart_playout();
            } else {
                control.skip_current();
            }
        }

        Ok(())
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.audio_frames += 1;
        self.audio_pts.push(frame.pts().unwrap_or(0));

        if frame.samples() > 0 {
            self.last_audio = Some((
                frame.pts().unwrap_or(0),
                frame.samples(),
                frame.plane::<f32>(0)[0],
            ));
        }

        Ok(())
    }

    fn reset_after_skip(&mut self, video_pts: i64, audio_pts: i64) -> Result<bool> {
        self.skip_target = Some((video_pts, audio_pts));

        Ok(self.reset_after_skip)
    }

    fn pad_audio(&mut self, samples: i64) -> Result<bool> {
        if self.virtual_audio_padding {
            self.padded_audio_samples += samples;

            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn test_live_receiver(rx: mpsc::Receiver<LiveEvent>) -> LiveReceiver {
    LiveReceiver {
        rx,
        pending_event: None,
        delay: super::delay::LiveDelay::new(Duration::ZERO, 25, 48_000),
        live_session: None,
        abort: Arc::new(AtomicBool::new(false)),
        channel_id: 0,
        fps: 25,
        sample_rate: 48_000,
        loudness_control: LiveLoudnessControl::new(false, LiveLoudnessConfig::default()),
        active: false,
        connecting: false,
        session_id: 0,
        session_output_start_seconds: None,
        session_source_start_seconds: None,
        pending_audio: VecDeque::new(),
        pending_audio_samples: 0,
        last_media_at: None,
        last_audio_at: None,
        last_video_frame: None,
        last_video_output_pts: None,
        last_audio_output_end_pts: None,
        file_resume_at_seconds: None,
        file_resume_shift_seconds: None,
        returned_to_file: false,
        video_pts: 0,
        audio_pts: 0,
        source_has_audio: false,
        listener_id: 0,
        loudness: LiveLoudnessProcessor::new(48_000, LiveLoudnessConfig::default()).ok(),
        benchmark: Arc::new(Mutex::new(None)),
    }
}

#[test]
fn multiple_listeners_select_priority_without_preempting_active_session() {
    let (high_tx, high_rx) = mpsc::channel();
    let (low_tx, low_rx) = mpsc::channel();
    let (out_tx, out_rx) = mpsc::sync_channel(8);
    let abort = Arc::new(AtomicBool::new(false));

    for (listener_id, tx) in [&high_tx, &low_tx].into_iter().enumerate() {
        tx.send(LiveEvent::Started {
            session_id: 1,
            has_audio: false,
            listener_id: listener_id as i32 + 1,
        })
        .unwrap();
    }
    high_tx
        .send(LiveEvent::Audio(1, frame::Audio::empty()))
        .unwrap();
    for tx in [&high_tx, &low_tx] {
        tx.send(LiveEvent::Video(1, frame::Video::empty())).unwrap();
    }

    let slots = [high_rx, low_rx]
        .into_iter()
        .map(|rx| ListenerSlot {
            receiver: test_live_receiver(rx),
            session: None,
            closed: false,
            pending_audio: VecDeque::new(),
            pending_audio_samples: 0,
        })
        .collect();
    let relay_abort = Arc::clone(&abort);
    let relay = thread::spawn(move || relay_live_listeners(slots, out_tx, relay_abort, 1));

    assert!(matches!(
        out_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        LiveEvent::Started {
            session_id: 1,
            listener_id: 1,
            ..
        }
    ));
    assert!(matches!(
        out_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        LiveEvent::Video(1, _)
    ));
    assert!(matches!(
        out_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        LiveEvent::Audio(1, _)
    ));

    high_tx.send(LiveEvent::Ended(1)).unwrap();
    assert!(matches!(
        out_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        LiveEvent::Ended(1)
    ));
    low_tx
        .send(LiveEvent::Video(1, frame::Video::empty()))
        .unwrap();
    assert!(matches!(
        out_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        LiveEvent::Started {
            session_id: 2,
            listener_id: 2,
            ..
        }
    ));
    assert!(matches!(
        out_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
        LiveEvent::Video(2, _)
    ));

    abort.store(true, Ordering::Relaxed);
    relay.join().unwrap();
}

#[test]
fn blocked_on_air_output_does_not_block_standby_listener() {
    let (high_tx, high_rx) = mpsc::channel();
    let (low_tx, low_rx) = mpsc::sync_channel(1);
    let (out_tx, out_rx) = mpsc::sync_channel(1);
    let abort = Arc::new(AtomicBool::new(false));
    out_tx.send(LiveEvent::Ended(999)).unwrap();
    high_tx
        .send(LiveEvent::Started {
            session_id: 1,
            has_audio: false,
            listener_id: 1,
        })
        .unwrap();
    high_tx
        .send(LiveEvent::Video(1, frame::Video::empty()))
        .unwrap();

    let slots = [test_live_receiver(high_rx), test_live_receiver(low_rx)]
        .into_iter()
        .map(|receiver| ListenerSlot {
            receiver,
            session: None,
            closed: false,
            pending_audio: VecDeque::new(),
            pending_audio_samples: 0,
        })
        .collect();
    let relay_abort = Arc::clone(&abort);
    let relay = thread::spawn(move || relay_live_listeners(slots, out_tx, relay_abort, 1));
    let (done_tx, done_rx) = mpsc::channel();
    let standby = thread::spawn(move || {
        if low_tx
            .send(LiveEvent::Started {
                session_id: 1,
                has_audio: false,
                listener_id: 2,
            })
            .is_err()
        {
            return;
        }

        for _ in 0..20 {
            if low_tx
                .send(LiveEvent::Video(1, frame::Video::empty()))
                .is_err()
            {
                return;
            }
        }

        let _ = done_tx.send(());
    });

    let drained = done_rx.recv_timeout(Duration::from_secs(1)).is_ok();
    let placeholder_received = matches!(
        out_rx.recv_timeout(Duration::from_secs(1)),
        Ok(LiveEvent::Ended(999))
    );
    let high_started = matches!(
        out_rx.recv_timeout(Duration::from_secs(1)),
        Ok(LiveEvent::Started { listener_id: 1, .. })
    );
    let high_video = matches!(
        out_rx.recv_timeout(Duration::from_secs(1)),
        Ok(LiveEvent::Video(1, _))
    );
    abort.store(true, Ordering::Relaxed);
    drop(out_rx);
    relay.join().unwrap();
    standby.join().unwrap();
    assert!(drained, "standby listener stalled behind on-air output");
    assert!(placeholder_received && high_started && high_video);
}

#[test]
#[ignore = "requires an FFmpeg CLI with libsrt and a local UDP socket"]
fn encrypted_srt_listener_accepts_an_mpegts_publisher() {
    use std::{net::UdpSocket, process::Command};

    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = socket.local_addr().unwrap().port();
    drop(socket);
    let url = format!("srt://127.0.0.1:{port}");
    let listener_options = BTreeMap::from([
        ("passphrase".to_string(), "secret-passphrase".to_string()),
        ("pbkeylen".to_string(), "16".to_string()),
    ]);
    let demuxer_options = BTreeMap::from([
        ("format".to_string(), "mpegts".to_string()),
        ("scan_all_pmts".to_string(), "1".to_string()),
        ("probesize".to_string(), "32768".to_string()),
    ]);
    let sender_url = format!("{url}?passphrase=secret-passphrase&pbkeylen=16");
    let abort = Arc::new(AtomicBool::new(false));
    let listener_abort = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let listener = thread::spawn({
        let abort = Arc::clone(&abort);
        let listener_abort = Arc::clone(&listener_abort);
        let url = url.clone();
        let options = listener_options.clone();
        let demuxer_options = demuxer_options.clone();
        move || {
            let result = super::live::open_live_listener(
                super::live::LiveInputBackend::Srt,
                &url,
                &options,
                &demuxer_options,
                abort,
                listener_abort,
            );

            match result {
                Ok(mut input) => {
                    let has_video = input
                        .streams()
                        .best(ffmpeg_next::media::Type::Video)
                        .is_some();
                    tx.send(Ok(has_video)).unwrap();

                    for _ in input.packets() {}
                }
                Err(error) => tx.send(Err(error.to_string())).unwrap(),
            }
        }
    });
    thread::sleep(Duration::from_millis(150));

    let mut sender = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-re",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=64x64:rate=25",
            "-t",
            "2",
            "-c:v",
            "mpeg2video",
            "-f",
            "mpegts",
            &sender_url,
        ])
        .spawn()
        .unwrap();
    let has_video = rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
    let status = sender.wait().unwrap();
    abort.store(true, Ordering::Relaxed);
    listener_abort.store(true, Ordering::Relaxed);
    assert!(status.success());
    assert!(has_video);
    listener.join().unwrap();
}

#[test]
#[ignore = "requires an FFmpeg CLI and a local TCP socket"]
fn rtmp_listener_can_force_live_flv_without_changing_the_default() {
    use std::{net::TcpListener, process::Command};

    for forced in [false, true] {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        let url = format!("rtmp://127.0.0.1:{port}/live/test");
        let demuxer_options = if forced {
            BTreeMap::from([("format".to_string(), "live_flv".to_string())])
        } else {
            BTreeMap::new()
        };
        let abort = Arc::new(AtomicBool::new(false));
        let listener_abort = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let listener = thread::spawn({
            let url = url.clone();
            let abort = Arc::clone(&abort);
            let listener_abort = Arc::clone(&listener_abort);
            move || {
                let result = super::live::open_live_listener(
                    super::live::LiveInputBackend::Rtmp,
                    &url,
                    &BTreeMap::new(),
                    &demuxer_options,
                    abort,
                    listener_abort,
                );

                match result {
                    Ok(mut input) => {
                        tx.send(Ok(input.format().name().to_string())).unwrap();

                        for _ in input.packets() {}
                    }
                    Err(error) => tx.send(Err(error.to_string())).unwrap(),
                }
            }
        });
        thread::sleep(Duration::from_millis(150));

        let mut sender = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=64x64:rate=25",
                "-t",
                "2",
                "-c:v",
                "flv1",
                "-f",
                "flv",
                &url,
            ])
            .spawn()
            .unwrap();
        let actual_format = rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
        let status = sender.wait().unwrap();
        abort.store(true, Ordering::Relaxed);
        listener_abort.store(true, Ordering::Relaxed);
        assert!(status.success());
        assert_eq!(actual_format, if forced { "live_flv" } else { "flv" });
        listener.join().unwrap();
    }
}

#[test]
fn startup_ring_buffer_accepts_late_video_without_reconnect() {
    let (tx, rx) = mpsc::channel();
    let mut live = test_live_receiver(rx);
    live.session_id = 1;
    live.connecting = true;

    for n in 0..600 {
        let mut audio = frame::Audio::new(
            ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
            1024,
            ffmpeg_next::ChannelLayout::STEREO,
        );
        audio.set_pts(Some(n * 1024));
        tx.send(LiveEvent::Audio(1, audio)).unwrap();
    }

    let mut output = CountingOutput::default();
    let control = crate::PlaybackControl::default();
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(live.pending_audio_samples <= 480_000);
    assert!(live.pending_audio.len() <= super::live::MAX_PENDING_AUDIO_FRAMES);
    assert!(live.pending_audio.front().unwrap().pts().unwrap() > 0);
    assert!(!control.live_active());
    let mut video = frame::Video::empty();
    video.set_pts(Some(320));
    tx.send(LiveEvent::Video(1, video)).unwrap();
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(control.live_active());
    assert!(live.pending_audio.is_empty());
}

#[test]
fn reader_limit_survives_listener_restarts_and_releases_slots() {
    let key = (Some(-101), "reader-limit-test".to_string());
    let first = super::live::LiveReaderPermit::try_acquire(&key).unwrap();
    let second = super::live::LiveReaderPermit::try_acquire(&key).unwrap();
    assert!(super::live::LiveReaderPermit::try_acquire(&key).is_none());
    assert!(super::live::LiveReaderPermit::try_acquire(&(Some(-102), key.1.clone())).is_some());
    let abort = Arc::new(AtomicBool::new(false));
    let worker_abort = abort.clone();
    let worker_key = key.clone();
    let worker =
        thread::spawn(move || super::live::LiveReaderPermit::acquire(worker_key, &worker_abort));
    abort.store(true, Ordering::Relaxed);
    assert!(worker.join().unwrap().is_none());
    drop(first);
    assert!(super::live::LiveReaderPermit::try_acquire(&key).is_some());
    drop(second);
    assert!(!super::live::LIVE_READERS.lock().unwrap().contains_key(&key));
}

#[test]
fn waiting_reader_recovers_when_a_slot_is_released() {
    let key = (Some(-103), "reader-recovery-test".to_string());
    let first = super::live::LiveReaderPermit::try_acquire(&key).unwrap();
    let second = super::live::LiveReaderPermit::try_acquire(&key).unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let permit = super::live::LiveReaderPermit::acquire(key, &AtomicBool::new(false));
        tx.send(permit).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(30)).is_err());
    drop(first);
    let recovered = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(recovered.is_some());
    worker.join().unwrap();
    drop(recovered);
    drop(second);
}

fn audio_frame(pts: i64, samples: usize) -> frame::Audio {
    let mut frame = frame::Audio::new(
        ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar),
        samples,
        ffmpeg_next::ChannelLayout::STEREO,
    );
    frame.set_rate(48_000);
    frame.set_pts(Some(pts));

    for channel in 0..2 {
        for (index, sample) in frame.plane_mut::<f32>(channel).iter_mut().enumerate() {
            *sample = index as f32;
        }
    }
    frame
}

#[test]
fn live_config_update_preserves_loudness_history_and_audio_timeline() {
    let (_tx, rx) = mpsc::channel();
    let mut live = test_live_receiver(rx);
    let config = LiveLoudnessConfig::default();
    live.loudness_control.update(true, config);
    let loudness_control = live.loudness_control.clone();
    let mut output = CountingOutput::default();
    let control = crate::PlaybackControl::default();
    let mut wrapper = LiveOverrideOutput::new(&mut output, &mut live, &control);
    wrapper.start_live_session(0.0);

    for (pts, samples) in [(0, 48_000 * 4), (48_000 * 4, 1_024)] {
        if pts > 0 {
            let before = loudness_control.metrics();
            assert!(before.short_term_lufs.is_some());
            assert!(before.rider_gain_db > 0.0);
            loudness_control.update(
                true,
                LiveLoudnessConfig {
                    target_lufs: -20.0,
                    ..config
                },
            );
            assert_eq!(loudness_control.metrics(), before);
        }

        let mut frame = audio_frame(pts, samples);

        for channel in 0..2 {
            for (index, sample) in frame.plane_mut::<f32>(channel).iter_mut().enumerate() {
                *sample = (std::f64::consts::TAU * 997.0 * (pts as usize + index) as f64 / 48_000.0)
                    .sin() as f32
                    * 0.01;
            }
        }

        wrapper.encode_live_audio_frame(frame).unwrap();
        assert_eq!(wrapper.live.audio_pts, pts + samples as i64);
        assert_eq!(wrapper.output.last_audio.unwrap().0, pts);
        assert!(loudness_control.metrics().short_term_lufs.is_some());
        assert!(loudness_control.metrics().rider_gain_db > 0.0);
    }

    assert_eq!(wrapper.output.audio_frames, 2);
    assert_eq!(wrapper.output.video_frames, 0);
}

#[test]
fn missing_audio_is_padded_and_late_audio_is_trimmed_without_shifting() {
    let (_tx, rx) = mpsc::channel();
    let mut live = test_live_receiver(rx);
    live.source_has_audio = true;
    let mut output = CountingOutput::default();
    let control = crate::PlaybackControl::default();
    let mut wrapper = LiveOverrideOutput::new(&mut output, &mut live, &control);
    wrapper.start_live_session(0.0);
    wrapper.live.active = true;
    wrapper.live.last_audio_at = Some(Instant::now());

    for pts in 0..25 {
        let mut frame = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 16, 16);
        frame.set_pts(Some(pts));
        wrapper.encode_live_video_frame(frame).unwrap();
    }
    assert_eq!(
        wrapper.live.audio_pts, 0,
        "valid audio is not pre-emptively replaced"
    );
    wrapper.live.last_audio_at = Some(Instant::now() - Duration::from_secs(3));
    wrapper.pad_missing_live_audio().unwrap();
    assert_eq!(wrapper.live.audio_pts, 48_000);
    wrapper
        .encode_live_audio_frame(audio_frame(0, 1024))
        .unwrap();
    assert_eq!(wrapper.live.audio_pts, 48_000); // stale audio is discarded
    wrapper
        .encode_live_audio_frame(audio_frame(47_500, 1024))
        .unwrap();
    assert_eq!(wrapper.live.audio_pts, 48_524);
    assert_eq!(wrapper.output.last_audio, Some((48_000, 524, 500.0)));
    // A subsequent audio dropout is padded too, not just startup.
    for pts in 25..50 {
        let mut frame = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 16, 16);
        frame.set_pts(Some(pts));
        wrapper.encode_live_video_frame(frame).unwrap();
    }
    assert_eq!(wrapper.live.audio_pts, 48_524);
    wrapper.live.last_audio_at = Some(Instant::now() - Duration::from_secs(3));
    wrapper.pad_missing_live_audio().unwrap();
    assert_eq!(wrapper.live.audio_pts, 96_000);
    wrapper
        .encode_live_audio_frame(audio_frame(96_000, 1024))
        .unwrap();
    assert_eq!(wrapper.output.last_audio, Some((96_000, 1024, 0.0)));
}

#[test]
fn timely_audio_is_not_replaced_by_silence() {
    let (_tx, rx) = mpsc::channel();
    let mut live = test_live_receiver(rx);
    live.source_has_audio = true;
    let mut output = CountingOutput::default();
    let control = crate::PlaybackControl::default();
    let mut wrapper = LiveOverrideOutput::new(&mut output, &mut live, &control);
    wrapper.start_live_session(0.0);

    for pts in 0..5 {
        let mut frame = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 16, 16);
        frame.set_pts(Some(pts));
        wrapper.encode_live_video_frame(frame).unwrap();
    }
    assert_eq!(wrapper.output.audio_frames, 0);
    wrapper
        .encode_live_audio_frame(audio_frame(0, 9600))
        .unwrap();
    assert_eq!(wrapper.output.last_audio, Some((0, 9600, 0.0)));
}

#[test]
fn millisecond_audio_timestamp_jitter_keeps_frames_contiguous() {
    let (_tx, rx) = mpsc::channel();
    let mut live = test_live_receiver(rx);
    live.source_has_audio = true;
    let mut output = CountingOutput::default();
    let control = crate::PlaybackControl::default();
    let mut wrapper = LiveOverrideOutput::new(&mut output, &mut live, &control);
    wrapper.start_live_session(0.0);

    // A 1024-sample AAC cadence cannot be represented exactly by FLV's
    // millisecond time base. These are typical rescaled PTS values.
    for pts in [0, 1008, 2064] {
        wrapper
            .encode_live_audio_frame(audio_frame(pts, 1024))
            .unwrap();
    }

    assert_eq!(wrapper.output.audio_frames, 3);
    assert_eq!(wrapper.output.last_audio, Some((2048, 1024, 0.0)));
    assert_eq!(wrapper.live.audio_pts, 3072);
}

#[test]
fn delayed_audio_uses_output_padding_without_a_silence_burst() {
    let (_tx, rx) = mpsc::channel();
    let mut live = test_live_receiver(rx);
    live.active = true;
    live.source_has_audio = true;
    live.last_audio_at = Some(Instant::now() - Duration::from_secs(3));
    let mut output = CountingOutput {
        virtual_audio_padding: true,
        ..CountingOutput::default()
    };
    let control = crate::PlaybackControl::default();
    let mut wrapper = LiveOverrideOutput::new(&mut output, &mut live, &control);
    wrapper.start_live_session(0.0);
    wrapper.live.video_pts = 75;

    wrapper.pad_missing_live_audio().unwrap();

    assert_eq!(wrapper.output.padded_audio_samples, 144_000);
    assert_eq!(wrapper.output.audio_frames, 0);
    assert_eq!(wrapper.live.audio_pts, 144_000);
}

#[test]
fn regression_live_padding_progresses_before_audio_start() {
    struct WaitingForAudio {
        video: mpsc::SyncSender<()>,
        audio: mpsc::Sender<()>,
    }
    impl FrameOutput for WaitingForAudio {
        fn audio_frame_size(&self) -> usize {
            1024
        }
        fn encode_video(&mut self, _: &frame::Video) -> Result<()> {
            self.video
                .send(())
                .map_err(|_| anyhow::anyhow!("test output closed"))
        }
        fn try_encode_video(&mut self, _: &frame::Video) -> Result<bool> {
            match self.video.try_send(()) {
                Ok(()) => Ok(true),
                Err(mpsc::TrySendError::Full(())) => Ok(false),
                Err(mpsc::TrySendError::Disconnected(())) => {
                    anyhow::bail!("test output closed")
                }
            }
        }
        fn encode_audio(&mut self, _: &frame::Audio) -> Result<()> {
            self.audio.send(())?;

            Ok(())
        }
        fn pad_audio(&mut self, _: i64) -> Result<bool> {
            self.audio.send(())?;

            Ok(true)
        }
    }

    let (tx, rx) = mpsc::channel();

    for pts in 0..32 {
        let mut video = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 4, 4);
        video.set_pts(Some(pts));
        tx.send(LiveEvent::Video(1, video)).unwrap();
    }

    let (video_tx, video_rx) = mpsc::sync_channel(8);
    let (audio_tx, audio_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        let mut live = test_live_receiver(rx);
        live.active = true;
        live.session_id = 1;
        live.source_has_audio = true;
        live.last_audio_at = Some(Instant::now());
        let mut output = WaitingForAudio {
            video: video_tx,
            audio: audio_tx,
        };
        let control = crate::PlaybackControl::default();
        let mut wrapper = LiveOverrideOutput::new(&mut output, &mut live, &control);
        wrapper.start_live_session(0.0);
        wrapper.wait_for_file_playback()
    });
    // Model a desktop renderer whose audio prebuffer has not started:
    // it cannot consume video until some audio or silence is supplied.
    let received_audio = audio_rx.recv_timeout(Duration::from_secs(4)).is_ok();
    // Always unblock and join the worker before asserting, including on failure.
    drop(video_rx);
    drop(tx);
    let _ = worker.join().expect("live test worker panicked");
    assert!(
        received_audio,
        "overdue silence was never supplied while video was backpressured"
    );
}

#[test]
fn timestamp_roundtrips_do_not_add_frames_or_samples() {
    for rate in [25, 30, 44_100, 48_000] {
        for pts in [7, 29, 103, 9999, 3_456_789_123] {
            assert_eq!(
                super::playback::seconds_to_pts(rate, pts as f64 / f64::from(rate)),
                pts
            );
        }
        assert_eq!(
            super::playback::seconds_to_pts(rate, 7.25 / f64::from(rate)),
            8
        );
    }
}

#[test]
fn startup_buffer_bounds_tiny_and_oversized_audio_frames() {
    for large in [false, true] {
        let (tx, rx) = mpsc::channel();
        let mut live = test_live_receiver(rx);
        live.session_id = 1;
        live.connecting = true;

        if large {
            tx.send(LiveEvent::Audio(1, audio_frame(0, 528_000)))
                .unwrap();
        } else {
            for pts in 0..600 {
                tx.send(LiveEvent::Audio(1, audio_frame(pts, 1))).unwrap();
            }
        }

        let mut output = CountingOutput::default();
        LiveOverrideOutput::new(&mut output, &mut live, &crate::PlaybackControl::default())
            .pump_live()
            .unwrap();
        assert!(live.pending_audio.len() <= super::live::MAX_PENDING_AUDIO_FRAMES);
        assert!(live.pending_audio_samples <= 480_000);
        assert!(live.pending_audio.front().unwrap().pts().unwrap() > 0);
        assert!(live.connecting);
    }
}

#[test]
fn live_limiter_reads_the_next_buffered_packet_and_keeps_audio_pts() {
    #[derive(Default)]
    struct CaptureOutput {
        audio: Vec<(i64, Vec<f32>)>,
    }

    impl FrameOutput for CaptureOutput {
        fn audio_frame_size(&self) -> usize {
            1_024
        }

        fn encode_video(&mut self, _: &frame::Video) -> Result<()> {
            Ok(())
        }

        fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
            self.audio
                .push((frame.pts().unwrap_or(0), frame.plane::<f32>(0).to_vec()));

            Ok(())
        }
    }

    let (tx, rx) = mpsc::channel();
    let mut live = test_live_receiver(rx);
    live.delay =
        super::delay::LiveDelay::new(Duration::from_millis(10), live.fps, live.sample_rate);
    live.loudness_control
        .update(true, LiveLoudnessConfig::default());
    let mut output = CaptureOutput::default();
    let control = crate::PlaybackControl::default();
    tx.send(LiveEvent::Started {
        session_id: 1,
        has_audio: true,
        listener_id: 1,
    })
    .unwrap();
    tx.send(LiveEvent::Video(
        1,
        frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 4, 4),
    ))
    .unwrap();
    let mut before = audio_frame(0, 1_024);
    let mut peak = audio_frame(1_024, 1_024);

    for channel in 0..2 {
        before.plane_mut::<f32>(channel).fill(0.1);
        peak.plane_mut::<f32>(channel).fill(0.1);
        peak.plane_mut::<f32>(channel)[0] = 2.0;
    }

    tx.send(LiveEvent::Audio(1, before)).unwrap();
    tx.send(LiveEvent::Audio(1, peak)).unwrap();
    tx.send(LiveEvent::Ended(1)).unwrap();
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(output.audio.is_empty());
    thread::sleep(Duration::from_millis(10));
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();

    assert_eq!(output.audio[0].0, 0);
    assert_eq!(output.audio[1].0, 1_024);
    assert_eq!(output.audio[0].1.len(), 1_024);
    assert_eq!(output.audio[1].1.len(), 1_024);
    assert_eq!(output.audio[0].1[0], 0.1);
    assert!(
        output.audio[0].1[1_000] < 0.06,
        "attenuation must precede the peak in the next packet"
    );
    assert!(output.audio[1].1[0] <= 10.0_f32.powf(-1.0 / 20.0));
    assert!(!control.live_active());
}

#[test]
fn delayed_live_takeover_keeps_audio_video_timing_and_drains_disconnected_tail() {
    let (tx, rx) = mpsc::channel();
    let mut live = test_live_receiver(rx);
    let latency = Duration::from_millis(10);
    live.delay = super::delay::LiveDelay::new(latency, live.fps, live.sample_rate);
    let control = crate::PlaybackControl::default();
    let mut output = CountingOutput::default();
    tx.send(LiveEvent::Started {
        session_id: 1,
        has_audio: true,
        listener_id: 1,
    })
    .unwrap();
    let mut video = frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 4, 4);
    video.set_pts(Some(25));
    tx.send(LiveEvent::Video(1, video)).unwrap();
    tx.send(LiveEvent::Audio(1, audio_frame(48_000, 1024)))
        .unwrap();

    assert!(
        !LiveOverrideOutput::new(&mut output, &mut live, &control)
            .pump_live()
            .unwrap()
    );
    assert!(!control.live_active());
    assert_eq!(output.video_frames, 0);
    assert_eq!(output.audio_frames, 0);
    thread::sleep(latency);
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(control.live_active());
    assert_eq!(live.last_video_output_pts, Some(0));
    assert_eq!(output.last_audio.unwrap().0, 0);
    assert_eq!(output.video_frames, 1);
    assert_eq!(output.audio_frames, 1);

    tx.send(LiveEvent::Audio(1, audio_frame(49_024, 1024)))
        .unwrap();
    tx.send(LiveEvent::Ended(1)).unwrap();
    drop(tx);
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(control.live_active());
    assert_eq!(output.audio_frames, 1);
    thread::sleep(latency);
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(!control.live_active());
    assert_eq!(&output.audio_pts[..2], &[0, 1024]);
    assert!(live.delay.is_empty());
}

#[test]
fn live_status_tracks_takeover_end_and_receiver_drop() {
    let (tx, rx) = mpsc::sync_channel(3);
    let mut live = test_live_receiver(rx);
    let control = crate::PlaybackControl::default();
    let mut output = CountingOutput::default();
    tx.send(LiveEvent::Started {
        session_id: 1,
        has_audio: true,
        listener_id: 1,
    })
    .unwrap();
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(
        !control.live_active(),
        "connecting alone must not block navigation"
    );
    tx.send(LiveEvent::Video(
        1,
        frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 4, 4),
    ))
    .unwrap();
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(control.live_active());
    assert_eq!(control.live_status(), (true, Some(1)));
    live.last_media_at = None;
    tx.send(LiveEvent::Ended(1)).unwrap();
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(!control.live_active());
    assert_eq!(control.live_status(), (false, None));
    tx.send(LiveEvent::Started {
        session_id: 2,
        has_audio: true,
        listener_id: 2,
    })
    .unwrap();
    tx.send(LiveEvent::Video(
        2,
        frame::Video::new(ffmpeg_next::format::Pixel::YUV420P, 4, 4),
    ))
    .unwrap();
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(control.live_active());
    assert_eq!(control.live_status(), (true, Some(2)));
    drop(live);
    assert_eq!(control.live_status(), (false, None));
    assert!(!control.live_active());
}

#[test]
fn navigation_reservation_preserves_first_live_frame_until_next_clip() {
    let (tx, rx) = mpsc::sync_channel(1);
    let mut live = test_live_receiver(rx);
    live.session_id = 1;
    live.source_has_audio = true;
    let control = crate::PlaybackControl::default();
    let navigation = control.begin_navigation().unwrap();
    let mut output = CountingOutput::default();
    tx.send(LiveEvent::Video(1, frame::Video::empty())).unwrap();
    let mut wrapper = LiveOverrideOutput::new(&mut output, &mut live, &control);
    wrapper.pump_live().unwrap();
    assert!(!control.live_active());
    navigation.commit();
    assert!(
        wrapper
            .pump_live()
            .unwrap_err()
            .is::<crate::playout::PlaybackSkipped>()
    );
    wrapper.reset_after_skip(0, 0).unwrap();
    wrapper.pump_live().unwrap();
    assert!(
        !control.live_active(),
        "skip cleanup must not activate live"
    );
    drop(wrapper);
    assert_eq!(output.video_frames, 0);
    LiveOverrideOutput::new(&mut output, &mut live, &control)
        .pump_live()
        .unwrap();
    assert!(control.live_active());
    assert_eq!(output.video_frames, 1);
}

#[test]
fn skip_cleanup_does_not_reenter_active_live_playback() {
    let (_tx, rx) = mpsc::sync_channel(1);
    let mut live = test_live_receiver(rx);
    live.active = true;
    live.video_pts = 250;
    live.audio_pts = 480_000;
    let mut output = CountingOutput::default();
    let control = crate::PlaybackControl::default();
    assert!(
        LiveOverrideOutput::new(&mut output, &mut live, &control)
            .reset_after_skip(25, 48_000)
            .unwrap()
    );
    assert_eq!(output.skip_target, None);
    assert_eq!(output.video_frames, 0);
    assert_eq!(output.audio_frames, 0);
    assert_eq!(live.video_pts, 250);
    assert_eq!(live.audio_pts, 480_000);
}

#[test]
fn active_live_wait_observes_skip_and_restart() {
    for restart in [false, true] {
        let (_tx, rx) = mpsc::sync_channel(1);
        let mut live = test_live_receiver(rx);
        live.active = true;
        live.last_media_at = Some(Instant::now());
        let control = crate::PlaybackControl::default();
        let worker_control = control.clone();
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));

            if restart {
                worker_control.restart_playout();
            } else {
                worker_control.skip_current();
            }
        });
        let mut output = CountingOutput::default();
        let result =
            LiveOverrideOutput::new(&mut output, &mut live, &control).wait_for_file_playback();
        worker.join().unwrap();
        let error = result.expect_err("active live playback must be interrupted");

        if restart {
            assert!(error.is::<crate::playout::PlaybackRestart>());
        } else {
            assert!(error.is::<crate::playout::PlaybackSkipped>());
        }
        assert!(live.active, "must interrupt before the live idle timeout");
    }
}

#[test]
fn busy_live_queue_observes_skip_and_restart_between_frames() {
    for restart in [false, true] {
        let (tx, rx) = mpsc::sync_channel(2);
        tx.send(LiveEvent::Video(1, frame::Video::empty())).unwrap();
        tx.send(LiveEvent::Video(1, frame::Video::empty())).unwrap();
        let mut live = test_live_receiver(rx);
        live.session_id = 1;
        live.active = true;
        live.source_has_audio = true;
        let control = crate::PlaybackControl::default();
        let mut output = CountingOutput {
            interrupt: Some((control.clone(), restart)),
            ..CountingOutput::default()
        };
        let error = LiveOverrideOutput::new(&mut output, &mut live, &control)
            .pump_live()
            .expect_err("must stop draining live frames on interruption");
        if restart {
            assert!(error.is::<crate::playout::PlaybackRestart>());
        } else {
            assert!(error.is::<crate::playout::PlaybackSkipped>());
        }
        assert_eq!(output.video_frames, 1);
    }
}

#[test]
fn passes_through_source_pts_before_resume_is_prepared() {
    assert_eq!(resume_pts(25, None, &mut None, 100, 40), 100);
}

#[test]
fn floors_source_pts_at_the_current_timeline_position() {
    assert_eq!(resume_pts(25, None, &mut None, 10, 40), 40);
}

#[test]
fn shifts_source_pts_to_the_resume_point_on_first_call() {
    let mut shift = None;
    // Resume at 10s into the file; the live source reports pts 0 (2s @ 25fps).
    let pts = resume_pts(25, Some(10.0), &mut shift, 0, 0);
    assert_eq!(pts, 250);
    assert_eq!(shift, Some(10.0));
}

#[test]
fn reuses_an_already_established_shift_for_subsequent_calls() {
    let mut shift = Some(5.0);
    // Even though resume_at_seconds now differs, an existing shift wins.
    let pts = resume_pts(48_000, Some(999.0), &mut shift, 48_000, 0);
    assert_eq!(pts, 48_000 * 6);
    assert_eq!(shift, Some(5.0));
}

#[test]
fn never_returns_pts_below_the_current_timeline_floor() {
    let mut shift = Some(-5.0);
    let pts = resume_pts(25, Some(1.0), &mut shift, 0, 1_000);
    assert_eq!(pts, 1_000);
}

#[test]
fn video_only_live_input_generates_silence() {
    let (_tx, rx) = mpsc::sync_channel(1);
    let mut live = test_live_receiver(rx);
    live.source_has_audio = false;
    let mut output = CountingOutput::default();
    let mut frame = frame::Video::empty();
    frame.set_pts(Some(0));

    LiveOverrideOutput::new(&mut output, &mut live, &crate::PlaybackControl::default())
        .encode_live_video_frame(frame)
        .unwrap();

    assert_eq!(output.video_frames, 1);
    assert!(output.audio_frames > 0);
}

#[test]
fn live_frame_sender_waits_until_full_channel_has_capacity() {
    let (tx, rx) = mpsc::sync_channel(1);
    tx.try_send(LiveEvent::Started {
        session_id: 1,
        has_audio: true,
        listener_id: 1,
    })
    .unwrap();
    let frame_seen = Arc::new(AtomicBool::new(false));
    let last_frame_ms = Arc::new(AtomicU64::new(u64::MAX));
    let abort = Arc::new(AtomicBool::new(false));
    let listener_abort = Arc::new(AtomicBool::new(false));
    let send_finished = Arc::new(AtomicBool::new(false));
    let worker_finished = Arc::clone(&send_finished);
    let worker = thread::spawn({
        let frame_seen = Arc::clone(&frame_seen);
        let last_frame_ms = Arc::clone(&last_frame_ms);
        let abort = Arc::clone(&abort);
        let listener_abort = Arc::clone(&listener_abort);
        move || {
            let mut sender = LiveFrameSender {
                tx,
                session_id: 1,
                last_frame_ms,
                frame_seen,
                abort,
                listener_abort,
                channel_id: 0,
            };
            sender
                .send_frame(LiveEvent::Video(1, frame::Video::empty()))
                .unwrap();
            worker_finished.store(true, Ordering::Relaxed);
        }
    });

    thread::sleep(Duration::from_millis(30));
    assert!(!send_finished.load(Ordering::Relaxed));

    assert!(frame_seen.load(Ordering::Relaxed));
    assert_ne!(last_frame_ms.load(Ordering::Relaxed), u64::MAX);
    assert!(matches!(
        rx.try_recv(),
        Ok(LiveEvent::Started { session_id: 1, .. })
    ));
    worker.join().unwrap();
    assert!(send_finished.load(Ordering::Relaxed));
    assert!(matches!(rx.try_recv(), Ok(LiveEvent::Video(1, _))));
    assert!(matches!(
        rx.try_recv(),
        Err(TryRecvError::Empty | TryRecvError::Disconnected)
    ));
}

#[test]
fn backpressure_does_not_make_the_live_watchdog_abort_the_reader() {
    let (tx, _rx) = mpsc::sync_channel(1);
    tx.try_send(LiveEvent::Started {
        session_id: 1,
        has_audio: true,
        listener_id: 1,
    })
    .unwrap();
    let abort = Arc::new(AtomicBool::new(false));
    let heartbeat = Arc::new(AtomicU64::new(super::live::monotonic_millis()));
    let watchdog = super::live::spawn_live_watchdog(
        Arc::clone(&heartbeat),
        Arc::new(AtomicBool::new(true)),
        Arc::clone(&abort),
        0,
        1,
    );
    let worker_abort = Arc::clone(&abort);
    let worker_heartbeat = Arc::clone(&heartbeat);
    let worker = thread::spawn(move || {
        super::live::send_live_event(
            &tx,
            LiveEvent::Ended(1),
            Some(&worker_abort),
            &AtomicBool::new(false),
            Some(&worker_heartbeat),
            "test",
            0,
        )
    });

    thread::sleep(super::live::LIVE_IDLE_TIMEOUT + Duration::from_millis(300));
    assert!(
        !abort.load(Ordering::Relaxed),
        "queue backpressure must not look like an idle publisher"
    );
    abort.store(true, Ordering::Relaxed);
    assert!(worker.join().unwrap().is_err());
    watchdog.join().unwrap();
}

#[test]
fn reader_shutdown_grace_accepts_a_delayed_exit() {
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        done_tx.send(Ok(())).unwrap();
    });

    assert!(super::live::wait_for_live_reader_exit(
        &done_rx,
        Duration::from_secs(1)
    ));
    worker.join().unwrap();
}

#[test]
fn reader_shutdown_grace_remains_bounded() {
    let (_done_tx, done_rx) = mpsc::sync_channel(1);
    let start = Instant::now();

    assert!(!super::live::wait_for_live_reader_exit(
        &done_rx,
        Duration::from_millis(20)
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn skip_reset_uses_the_actual_output_timeline() {
    let (_tx, rx) = mpsc::sync_channel(1);
    let mut live = test_live_receiver(rx);
    live.video_pts = 50;
    live.audio_pts = 95_000;
    live.last_video_output_pts = Some(49);
    live.last_audio_output_end_pts = Some(95_000);
    let mut output = CountingOutput {
        reset_after_skip: true,
        ..CountingOutput::default()
    };

    let reset = LiveOverrideOutput::new(&mut output, &mut live, &crate::PlaybackControl::default())
        .reset_after_skip(1_000, 2_000)
        .unwrap();

    assert!(reset);
    assert_eq!(output.skip_target, Some((50, 96_000)));
    assert_eq!(live.video_pts, 50);
    assert_eq!(live.audio_pts, 96_000);
    assert_eq!(live.last_video_output_pts, None);
    assert_eq!(live.last_audio_output_end_pts, Some(96_000));
}

#[test]
fn live_frame_sender_stops_waiting_when_aborted() {
    let (tx, _rx) = mpsc::sync_channel(1);
    tx.try_send(LiveEvent::Started {
        session_id: 1,
        has_audio: true,
        listener_id: 1,
    })
    .unwrap();
    let abort = Arc::new(AtomicBool::new(true));
    let mut sender = LiveFrameSender {
        tx,
        session_id: 1,
        last_frame_ms: Arc::new(AtomicU64::new(0)),
        frame_seen: Arc::new(AtomicBool::new(false)),
        abort,
        listener_abort: Arc::new(AtomicBool::new(false)),
        channel_id: 0,
    };

    assert!(
        sender
            .send_frame(LiveEvent::Video(1, frame::Video::empty()))
            .is_err()
    );
}

#[test]
fn live_frame_sender_reports_disconnected_channel() {
    let (tx, rx) = mpsc::sync_channel(1);
    drop(rx);
    let mut sender = LiveFrameSender {
        tx,
        session_id: 1,
        last_frame_ms: Arc::new(AtomicU64::new(0)),
        frame_seen: Arc::new(AtomicBool::new(false)),
        abort: Arc::new(AtomicBool::new(false)),
        listener_abort: Arc::new(AtomicBool::new(false)),
        channel_id: 0,
    };

    assert!(
        sender
            .send_frame(LiveEvent::Video(1, frame::Video::empty()))
            .is_err()
    );
}

#[test]
fn pump_live_ignores_frames_from_stale_sessions() {
    let (tx, rx) = mpsc::sync_channel(live_channel_capacity(25));
    tx.send(LiveEvent::Video(2, frame::Video::empty())).unwrap();
    tx.send(LiveEvent::Audio(2, frame::Audio::empty())).unwrap();
    tx.send(LiveEvent::Ended(2)).unwrap();
    let mut live = test_live_receiver(rx);
    live.session_id = 1;
    live.active = true;
    live.last_media_at = Some(Instant::now());
    let mut output = CountingOutput::default();

    let received_event = super::live::LiveOverrideOutput::new(
        &mut output,
        &mut live,
        &crate::PlaybackControl::default(),
    )
    .pump_live()
    .unwrap();

    assert!(!received_event);
    assert!(live.active);
    assert_eq!(output.video_frames, 0);
    assert_eq!(output.audio_frames, 0);
}
