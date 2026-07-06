use std::{
    error::Error,
    ffi::{CStr, CString},
    fmt, ptr,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use ffmpeg_next::{
    Dictionary, Error as FfmpegError, ffi, format, frame,
    util::{
        channel_layout::ChannelLayout,
        format::sample::{Sample, Type as SampleType},
        interrupt,
    },
};
use log::{error, info, warn};

use crate::{
    output::FrameOutput,
    playout::{InputPlaybackOptions, LogoFadePlan, Timeline, play_opened_input},
    utils::{config::OutputConfig, logging},
};

const LIVE_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const LIVE_IDLE_TIMEOUT: Duration = Duration::from_secs(3);
const LIVE_WATCHDOG_INTERVAL: Duration = Duration::from_millis(100);

/// Number of RTMP reader threads that outlived their `abort` signal and are
/// being reaped in the background. Exposed only via log messages for now;
/// see the usage in `run_rtmp_listener` for context.
static STUCK_LIVE_WORKERS: AtomicU64 = AtomicU64::new(0);

pub struct LiveReceiver {
    rx: Receiver<LiveEvent>,
    fps: u32,
    sample_rate: u32,
    active: bool,
    connecting: bool,
    session_id: u64,
    session_output_start_seconds: Option<f64>,
    session_source_start_seconds: Option<f64>,
    pending_audio: Vec<frame::Audio>,
    last_media_at: Option<Instant>,
    last_video_frame: Option<frame::Video>,
    last_video_output_pts: Option<i64>,
    last_audio_output_end_pts: Option<i64>,
    file_resume_at_seconds: Option<f64>,
    file_resume_shift_seconds: Option<f64>,
    returned_to_file: bool,
    video_pts: i64,
    audio_pts: i64,
}

#[derive(Debug)]
pub(crate) struct LiveEnded;

impl fmt::Display for LiveEnded {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("live input ended")
    }
}

impl Error for LiveEnded {}

enum LiveEvent {
    Started(u64),
    Video(u64, frame::Video),
    Audio(u64, frame::Audio),
    Ended(u64),
}

pub fn spawn_rtmp_listener(url: String, cfg: OutputConfig) -> LiveReceiver {
    let fps = cfg.fps;
    let sample_rate = cfg.sample_rate;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || run_rtmp_listener(url, cfg, tx));

    LiveReceiver {
        rx,
        fps,
        sample_rate,
        active: false,
        connecting: false,
        session_id: 0,
        session_output_start_seconds: None,
        session_source_start_seconds: None,
        pending_audio: Vec::new(),
        last_media_at: None,
        last_video_frame: None,
        last_video_output_pts: None,
        last_audio_output_end_pts: None,
        file_resume_at_seconds: None,
        file_resume_shift_seconds: None,
        returned_to_file: false,
        video_pts: 0,
        audio_pts: 0,
    }
}

pub(crate) struct LiveOverrideOutput<'a, O: FrameOutput> {
    output: &'a mut O,
    live: &'a mut LiveReceiver,
}

impl<'a, O: FrameOutput> LiveOverrideOutput<'a, O> {
    pub(crate) fn new(output: &'a mut O, live: &'a mut LiveReceiver) -> Self {
        Self { output, live }
    }

    fn pump_live(&mut self) -> Result<bool> {
        let mut received_event = false;
        loop {
            match self.live.rx.try_recv() {
                Ok(LiveEvent::Started(session_id)) => {
                    self.live.session_id = session_id;
                    self.live.session_output_start_seconds = None;
                    self.live.session_source_start_seconds = None;
                    self.live.pending_audio.clear();
                    self.live.last_media_at = Some(Instant::now());
                    self.live.active = false;
                    self.live.connecting = true;
                    info!("live input connected; waiting for first video frame");
                }
                Ok(LiveEvent::Video(session_id, frame)) => {
                    if session_id == self.live.session_id {
                        if !self.live.active {
                            info!("first live video frame received; switching to RTMP live");
                            self.live.active = true;
                            self.live.connecting = false;
                            self.start_live_session(video_seconds(
                                self.live.fps,
                                frame.pts().unwrap_or(0),
                            ));
                        }
                        received_event = true;
                        self.encode_live_video_frame(frame)?;
                        self.flush_pending_audio()?;
                        self.live.last_media_at = Some(Instant::now());
                    }
                }
                Ok(LiveEvent::Audio(session_id, frame)) => {
                    if session_id == self.live.session_id {
                        received_event = true;
                        self.live.last_media_at = Some(Instant::now());
                        if self.live.active {
                            self.encode_live_audio_frame(frame)?;
                        } else {
                            self.live.pending_audio.push(frame);
                        }
                    }
                }
                Ok(LiveEvent::Ended(session_id)) => {
                    if session_id == self.live.session_id {
                        info!("live input ended; switching back to file playback");
                        if self.live.active {
                            self.fill_live_gap_since_last_media()?;
                            self.align_live_pts_to_common_time();
                            self.prepare_file_resume();
                            self.live.returned_to_file = true;
                        }
                        self.live.active = false;
                        self.live.connecting = false;
                        self.live.pending_audio.clear();
                    }
                }
                Err(TryRecvError::Empty) => return Ok(received_event),
                Err(TryRecvError::Disconnected) => {
                    if self.live.active {
                        self.fill_live_gap_since_last_media()?;
                        self.align_live_pts_to_common_time();
                        self.prepare_file_resume();
                        self.live.returned_to_file = true;
                    }
                    self.live.active = false;
                    self.live.connecting = false;
                    self.live.pending_audio.clear();
                    return Ok(received_event);
                }
            }
        }
    }

    fn wait_for_file_playback(&mut self) -> Result<()> {
        self.pump_live()?;
        while self.live.active || self.live.connecting {
            thread::sleep(Duration::from_millis(10));
            self.pump_live()?;
            let idle_for = self
                .live
                .last_media_at
                .map(|last_media_at| last_media_at.elapsed())
                .unwrap_or_default();
            let timeout = if self.live.connecting {
                LIVE_STARTUP_TIMEOUT
            } else {
                LIVE_IDLE_TIMEOUT
            };
            if idle_for >= timeout {
                if self.live.active {
                    info!("live input idle; switching back to file playback");
                    self.fill_live_gap(idle_for)?;
                    self.align_live_pts_to_common_time();
                    self.prepare_file_resume();
                    self.live.returned_to_file = true;
                } else {
                    info!("live input produced no video frame; switching back to file playback");
                }
                self.live.active = false;
                self.live.connecting = false;
                self.live.pending_audio.clear();
            }
        }
        if self.live.returned_to_file {
            self.live.returned_to_file = false;
            return Err(LiveEnded.into());
        }
        Ok(())
    }

    fn fill_live_gap_since_last_media(&mut self) -> Result<()> {
        if let Some(last_media_at) = self.live.last_media_at {
            self.fill_live_gap(last_media_at.elapsed())?;
        }
        Ok(())
    }

    fn fill_live_gap(&mut self, duration: Duration) -> Result<()> {
        let video_frames = (duration.as_secs_f64() * f64::from(self.live.fps)).ceil() as i64;
        if let Some(last_video_frame) = self.live.last_video_frame.clone() {
            for _ in 0..video_frames {
                let mut frame = last_video_frame.clone();
                frame.set_pts(Some(self.live.video_pts));
                self.output.encode_video(&frame)?;
                self.remember_video_frame(frame, self.live.video_pts);
                self.live.video_pts += 1;
            }
        } else {
            self.live.video_pts += video_frames;
        }

        let mut remaining_samples =
            (duration.as_secs_f64() * f64::from(self.live.sample_rate)).ceil() as usize;
        let frame_size = self.output.audio_frame_size().max(1);
        while remaining_samples > 0 {
            let samples = remaining_samples.min(frame_size);
            let mut frame = frame::Audio::new(
                Sample::F32(SampleType::Planar),
                samples,
                ChannelLayout::STEREO,
            );
            frame.set_rate(self.live.sample_rate);
            frame.set_pts(Some(self.live.audio_pts));
            for channel in 0..2 {
                for sample in frame.plane_mut::<f32>(channel) {
                    *sample = 0.0;
                }
            }
            self.output.encode_audio(&frame)?;
            self.remember_audio_frame_end(self.live.audio_pts + samples as i64);
            remaining_samples -= samples;
        }

        self.live.last_media_at = Some(Instant::now());
        Ok(())
    }

    fn fill_video_until(&mut self, next_pts: i64) -> Result<()> {
        let Some(mut fill_pts) = self.live.last_video_output_pts.map(|pts| pts + 1) else {
            return Ok(());
        };
        let Some(last_video_frame) = self.live.last_video_frame.clone() else {
            return Ok(());
        };

        while fill_pts < next_pts {
            let mut frame = last_video_frame.clone();
            frame.set_pts(Some(fill_pts));
            self.output.encode_video(&frame)?;
            self.remember_video_frame(frame, fill_pts);
            fill_pts += 1;
        }

        Ok(())
    }

    fn fill_audio_until(&mut self, next_pts: i64) -> Result<()> {
        let Some(mut fill_pts) = self.live.last_audio_output_end_pts else {
            return Ok(());
        };
        if fill_pts >= next_pts {
            return Ok(());
        }

        let frame_size = self.output.audio_frame_size().max(1);
        while fill_pts < next_pts {
            let samples = (next_pts - fill_pts).min(frame_size as i64) as usize;
            let mut frame = frame::Audio::new(
                Sample::F32(SampleType::Planar),
                samples,
                ChannelLayout::STEREO,
            );
            frame.set_rate(self.live.sample_rate);
            frame.set_pts(Some(fill_pts));
            for channel in 0..2 {
                for sample in frame.plane_mut::<f32>(channel) {
                    *sample = 0.0;
                }
            }
            self.output.encode_audio(&frame)?;
            fill_pts += samples as i64;
            self.remember_audio_frame_end(fill_pts);
        }

        Ok(())
    }

    fn remember_video_frame(&mut self, frame: frame::Video, pts: i64) {
        self.live.last_video_frame = Some(frame);
        self.live.last_video_output_pts = Some(pts);
    }

    fn remember_audio_frame_end(&mut self, end_pts: i64) {
        self.live.audio_pts = end_pts;
        self.live.last_audio_output_end_pts = Some(end_pts);
    }

    fn start_live_session(&mut self, source_start_seconds: f64) {
        let output_start_seconds = self.common_live_seconds();
        self.live.video_pts = self
            .live
            .video_pts
            .max(seconds_to_video_pts(self.live.fps, output_start_seconds));
        self.live.audio_pts = self.live.audio_pts.max(seconds_to_audio_pts(
            self.live.sample_rate,
            output_start_seconds,
        ));
        self.live.session_output_start_seconds = Some(output_start_seconds);
        self.live.session_source_start_seconds = Some(source_start_seconds);
    }

    fn common_live_seconds(&self) -> f64 {
        let video_seconds = self.live.video_pts as f64 / f64::from(self.live.fps);
        let audio_seconds = self.live.audio_pts as f64 / f64::from(self.live.sample_rate);
        video_seconds.max(audio_seconds)
    }

    fn live_output_seconds(&self, source_seconds: f64) -> f64 {
        let output_start = self
            .live
            .session_output_start_seconds
            .unwrap_or_else(|| self.common_live_seconds());
        let source_start = self
            .live
            .session_source_start_seconds
            .unwrap_or(source_seconds);
        output_start + (source_seconds - source_start)
    }

    fn encode_live_video_frame(&mut self, mut frame: frame::Video) -> Result<()> {
        let source_pts = frame.pts().unwrap_or(0);
        let pts = seconds_to_video_pts(
            self.live.fps,
            self.live_output_seconds(video_seconds(self.live.fps, source_pts)),
        )
        .max(self.live.video_pts);
        self.fill_video_until(pts)?;
        frame.set_pts(Some(pts));
        self.output.encode_video(&frame)?;
        self.remember_video_frame(frame, pts);
        self.live.video_pts = pts + 1;
        Ok(())
    }

    fn encode_live_audio_frame(&mut self, mut frame: frame::Audio) -> Result<()> {
        let samples = frame.samples() as i64;
        let source_pts = frame.pts().unwrap_or(0);
        let pts = seconds_to_audio_pts(
            self.live.sample_rate,
            self.live_output_seconds(audio_seconds(self.live.sample_rate, source_pts)),
        )
        .max(self.live.audio_pts);
        self.fill_audio_until(pts)?;
        frame.set_pts(Some(pts));
        self.output.encode_audio(&frame)?;
        self.remember_audio_frame_end(pts + samples);
        Ok(())
    }

    fn flush_pending_audio(&mut self) -> Result<()> {
        let pending = std::mem::take(&mut self.live.pending_audio);
        for frame in pending {
            self.encode_live_audio_frame(frame)?;
        }
        Ok(())
    }

    fn align_live_pts_to_common_time(&mut self) {
        let common_seconds = self.common_live_seconds();
        self.live.video_pts = self
            .live
            .video_pts
            .max(seconds_to_video_pts(self.live.fps, common_seconds));
        self.live.audio_pts = self
            .live
            .audio_pts
            .max(seconds_to_audio_pts(self.live.sample_rate, common_seconds));
    }

    fn prepare_file_resume(&mut self) {
        let video_seconds = self.live.video_pts as f64 / f64::from(self.live.fps);
        let audio_seconds = self.live.audio_pts as f64 / f64::from(self.live.sample_rate);
        self.live.file_resume_at_seconds = Some(video_seconds.max(audio_seconds));
        self.live.file_resume_shift_seconds = None;
    }

    fn file_video_pts(&mut self, source_pts: i64) -> i64 {
        resume_pts(
            self.live.fps,
            self.live.file_resume_at_seconds,
            &mut self.live.file_resume_shift_seconds,
            source_pts,
            self.live.video_pts,
        )
    }

    fn file_audio_pts(&mut self, source_pts: i64) -> i64 {
        resume_pts(
            self.live.sample_rate,
            self.live.file_resume_at_seconds,
            &mut self.live.file_resume_shift_seconds,
            source_pts,
            self.live.audio_pts,
        )
    }
}

/// Shared resume-pts computation for both video and audio.
///
/// `resume_shift_seconds` is intentionally a single value shared between the
/// video and audio streams: whichever stream resumes first fixes the shift,
/// and the other stream reuses it so both tracks stay aligned to the same
/// point in the file.
fn resume_pts(
    rate: u32,
    resume_at_seconds: Option<f64>,
    resume_shift_seconds: &mut Option<f64>,
    source_pts: i64,
    floor_pts: i64,
) -> i64 {
    if let Some(resume_seconds) = resume_at_seconds {
        let source_seconds = source_pts as f64 / f64::from(rate);
        let shift_seconds = *resume_shift_seconds.get_or_insert(resume_seconds - source_seconds);
        ((source_seconds + shift_seconds) * f64::from(rate)).round() as i64
    } else {
        source_pts.max(floor_pts)
    }
    .max(floor_pts)
}

fn video_seconds(fps: u32, pts: i64) -> f64 {
    pts as f64 / f64::from(fps)
}

fn audio_seconds(sample_rate: u32, pts: i64) -> f64 {
    pts as f64 / f64::from(sample_rate)
}

fn seconds_to_video_pts(fps: u32, seconds: f64) -> i64 {
    (seconds * f64::from(fps)).ceil() as i64
}

fn seconds_to_audio_pts(sample_rate: u32, seconds: f64) -> i64 {
    (seconds * f64::from(sample_rate)).ceil() as i64
}

impl<O: FrameOutput> FrameOutput for LiveOverrideOutput<'_, O> {
    fn audio_frame_size(&self) -> usize {
        self.output.audio_frame_size()
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        if !self.live.active
            && let Some(pts) = frame.pts()
        {
            self.live.video_pts = self.live.video_pts.max(pts);
        }
        self.wait_for_file_playback()?;

        let mut frame = frame.clone();
        let pts = self.file_video_pts(frame.pts().unwrap_or(self.live.video_pts));
        self.fill_video_until(pts)?;
        frame.set_pts(Some(pts));
        self.output.encode_video(&frame)?;
        self.remember_video_frame(frame, pts);
        self.live.video_pts = pts + 1;
        Ok(())
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        if !self.live.active
            && let Some(pts) = frame.pts()
        {
            self.live.audio_pts = self.live.audio_pts.max(pts);
        }
        self.wait_for_file_playback()?;

        let mut frame = frame.clone();
        let samples = frame.samples() as i64;
        let pts = self.file_audio_pts(frame.pts().unwrap_or(self.live.audio_pts));
        self.fill_audio_until(pts)?;
        frame.set_pts(Some(pts));
        self.output.encode_audio(&frame)?;
        self.remember_audio_frame_end(pts + samples);
        Ok(())
    }

    fn set_video_end(&mut self, video_end_pts: Option<i64>) -> Result<()> {
        self.output.set_video_end(video_end_pts)
    }

    fn video_finished(&mut self) -> Result<()> {
        self.output.video_finished()
    }

    fn write_vtt_subtitles(
        &mut self,
        media_path: &str,
        output_start_ms: i64,
        source_start_ms: i64,
    ) -> Result<()> {
        self.output
            .write_vtt_subtitles(media_path, output_start_ms, source_start_ms)
    }
}

struct LiveFrameSender {
    tx: Sender<LiveEvent>,
    session_id: u64,
    last_frame_ms: Arc<AtomicU64>,
    frame_seen: Arc<AtomicBool>,
}

impl FrameOutput for LiveFrameSender {
    fn audio_frame_size(&self) -> usize {
        1024
    }

    fn encode_video(&mut self, frame: &frame::Video) -> Result<()> {
        self.frame_seen.store(true, Ordering::Relaxed);
        self.last_frame_ms
            .store(monotonic_millis(), Ordering::Relaxed);
        self.tx
            .send(LiveEvent::Video(self.session_id, frame.clone()))
            .context("failed to send live video frame")
    }

    fn encode_audio(&mut self, frame: &frame::Audio) -> Result<()> {
        self.frame_seen.store(true, Ordering::Relaxed);
        self.last_frame_ms
            .store(monotonic_millis(), Ordering::Relaxed);
        self.tx
            .send(LiveEvent::Audio(self.session_id, frame.clone()))
            .context("failed to send live audio frame")
    }
}

fn run_rtmp_listener(url: String, cfg: OutputConfig, tx: Sender<LiveEvent>) {
    let mut session_id = 0;

    loop {
        let abort = Arc::new(AtomicBool::new(false));

        match logging::with_ingest_logs(cfg.channel_id, || {
            open_rtmp_listener(&url, Arc::clone(&abort))
        }) {
            Ok(ictx) => {
                session_id += 1;
                let last_frame_ms = Arc::new(AtomicU64::new(monotonic_millis()));
                let frame_seen = Arc::new(AtomicBool::new(false));
                let watchdog = spawn_live_watchdog(
                    Arc::clone(&last_frame_ms),
                    Arc::clone(&frame_seen),
                    Arc::clone(&abort),
                );

                if tx.send(LiveEvent::Started(session_id)).is_err() {
                    abort.store(true, Ordering::Relaxed);
                    let _ = watchdog.join();
                    return;
                }

                let (done_tx, done_rx) = mpsc::sync_channel(1);
                let mut output = LiveFrameSender {
                    tx: tx.clone(),
                    session_id,
                    last_frame_ms,
                    frame_seen,
                };

                let worker_url = url.clone();
                let worker_cfg = cfg.clone();
                let worker = thread::spawn(move || {
                    let mut timeline = Timeline::new();
                    let logo_fade_plan = LogoFadePlan::none(timeline.video_pts(), &worker_cfg);
                    let result = logging::with_ingest_logs(worker_cfg.channel_id, || {
                        play_opened_input(
                            &worker_url,
                            ictx,
                            &worker_cfg,
                            &mut timeline,
                            &mut output,
                            InputPlaybackOptions {
                                seek_seconds: None,
                                duration_seconds: None,
                                subtitles_media_path: None,
                                logo_fade_plan,
                            },
                        )
                    });
                    let _ = done_tx.send(result.map_err(|error| format!("{error:#}")));
                });

                let mut worker_finished = false;
                while !abort.load(Ordering::Relaxed) {
                    if let Ok(result) = done_rx.try_recv() {
                        worker_finished = true;
                        if let Err(error) = result {
                            error!("live input failed: {error}");
                        }
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }

                abort.store(true, Ordering::Relaxed);
                let _ = watchdog.join();
                if worker_finished {
                    let _ = worker.join();
                } else {
                    // The interrupt callback only aborts FFmpeg I/O between reads; if the
                    // worker is blocked in a single long-running syscall it may not exit
                    // promptly. Rather than block the listener loop on `worker.join()`,
                    // reap it in the background so a stuck reader is still observed (and
                    // its thread reclaimed) once it eventually unblocks or errors out.
                    let stuck_count = STUCK_LIVE_WORKERS.fetch_add(1, Ordering::Relaxed) + 1;
                    warn!(
                        "live input reader is still blocked; restarting ingest server without waiting ({stuck_count} stuck reader(s) pending cleanup)"
                    );
                    thread::spawn(move || {
                        let _ = worker.join();
                        let remaining = STUCK_LIVE_WORKERS.fetch_sub(1, Ordering::Relaxed) - 1;
                        info!(
                            "previously stuck live input reader exited ({remaining} stuck reader(s) still pending)"
                        );
                    });
                }

                info!("Restart ingest server after live input ended");
                if tx.send(LiveEvent::Ended(session_id)).is_err() {
                    return;
                }
            }
            Err(error) => {
                abort.store(true, Ordering::Relaxed);
                error!("RTMP listener failed: {error:#}; retrying");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn spawn_live_watchdog(
    last_frame_ms: Arc<AtomicU64>,
    frame_seen: Arc<AtomicBool>,
    abort: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while !abort.load(Ordering::Relaxed) {
            thread::sleep(LIVE_WATCHDOG_INTERVAL);

            let last_frame_ms = last_frame_ms.load(Ordering::Relaxed);
            let timeout = if frame_seen.load(Ordering::Relaxed) {
                LIVE_IDLE_TIMEOUT
            } else {
                LIVE_STARTUP_TIMEOUT
            };

            if monotonic_millis().saturating_sub(last_frame_ms) >= timeout.as_millis() as u64 {
                if frame_seen.load(Ordering::Relaxed) {
                    info!("live input disconnected or idle; restarting ingest server");
                } else {
                    info!("live input produced no decodable frames; restarting ingest server");
                }
                abort.store(true, Ordering::Relaxed);
                return;
            }
        }
    })
}

fn open_rtmp_listener(url: &str, abort: Arc<AtomicBool>) -> Result<format::context::Input> {
    let mut options = Dictionary::new();
    options.set("listen", "1");
    options.set("timeout", "0");
    logging::clear_unexpected_rtmp_stream();

    let url_cstring =
        CString::new(url).with_context(|| format!("RTMP input URL contains NUL byte: {url}"))?;
    let interrupt_abort = Arc::clone(&abort);
    let interrupt = interrupt::new(Box::new(move || interrupt_abort.load(Ordering::Relaxed)));

    // `ffmpeg-next`'s safe `format::input_with*` helpers open a file/stream and
    // return only once the input is fully ready; they provide no way to attach
    // an interrupt callback before `avformat_open_input` starts blocking in
    // "listen" mode (which waits for an incoming RTMP publisher). Reaching the
    // interrupt callback and swapping the auto-allocated dictionary back out
    // requires driving the C API directly.
    unsafe {
        let mut ps = ffi::avformat_alloc_context();
        if ps.is_null() {
            anyhow::bail!("failed to allocate RTMP input context");
        }

        (*ps).interrupt_callback = interrupt.interrupt;

        let mut opts = options.disown();
        let open_result =
            ffi::avformat_open_input(&mut ps, url_cstring.as_ptr(), ptr::null_mut(), &mut opts);
        Dictionary::own(opts);

        if open_result != 0 {
            ffi::avformat_close_input(&mut ps);
            return Err(FfmpegError::from(open_result))
                .with_context(|| format!("failed to listen for RTMP input at {url}"));
        }

        let stream_info_result = ffi::avformat_find_stream_info(ps, ptr::null_mut());
        if stream_info_result < 0 {
            ffi::avformat_close_input(&mut ps);
            return Err(FfmpegError::from(stream_info_result))
                .with_context(|| format!("failed to read RTMP stream info at {url}"));
        }

        if let Some((actual_key, expected_key)) = logging::take_unexpected_rtmp_stream() {
            ffi::avformat_close_input(&mut ps);
            anyhow::bail!(
                "incoming RTMP stream key {actual_key:?} does not match configured key {expected_key:?}"
            );
        }

        if let Some(expected_key) = rtmp_stream_key(url)
            && let Some(actual_key) = rtmp_context_option(ps, "rtmp_playpath")
            && actual_key != expected_key
        {
            ffi::avformat_close_input(&mut ps);
            anyhow::bail!(
                "incoming RTMP stream key {actual_key:?} does not match configured key {expected_key:?}"
            );
        }

        Ok(format::context::Input::wrap(ps))
    }
}

fn rtmp_stream_key(url: &str) -> Option<String> {
    let path = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url)
        .split_once('/')
        .map(|(_, path)| path)?;

    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|key| !key.is_empty())
        .map(str::to_string)
}

unsafe fn rtmp_context_option(ps: *mut ffi::AVFormatContext, name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut value = std::ptr::null_mut();

    let candidates = [
        ps.cast(),
        (!ps.is_null()).then(|| unsafe { (*ps).pb.cast() })?,
        (!ps.is_null() && !unsafe { (*ps).pb }.is_null()).then(|| unsafe { (*(*ps).pb).opaque })?,
    ];

    for candidate in candidates {
        if candidate.is_null() {
            continue;
        }
        let result = unsafe {
            ffi::av_opt_get(
                candidate,
                name.as_ptr(),
                ffi::AV_OPT_SEARCH_CHILDREN,
                &mut value,
            )
        };
        if result >= 0 && !value.is_null() {
            let option = unsafe { CStr::from_ptr(value.cast()) }
                .to_string_lossy()
                .to_string();
            unsafe { ffi::av_free(value.cast()) };
            return (!option.is_empty()).then_some(option);
        }
    }

    None
}

/// Monotonic millisecond clock used for idle-timeout tracking.
///
/// Uses `Instant` (relative to a fixed process-lifetime epoch) instead of
/// `SystemTime`/`UNIX_EPOCH` so that system clock adjustments (e.g. NTP jumps)
/// cannot cause the live watchdog to misfire.
fn monotonic_millis() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    epoch.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::resume_pts;

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
}
