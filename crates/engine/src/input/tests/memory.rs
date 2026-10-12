//! Opt-in RTMP resource tests. Run alone: RSS belongs to the whole test process.

use std::{
    env, fs,
    io::Write,
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail, ensure};

use crate::{
    OutputConfig, PlaybackControl, StreamType,
    input::live::{LiveIdleTimeout, LiveReceiver, live_reader_counts, spawn_rtmp_listener},
    output::{FrameOutput, Output},
};

use super::LiveOverrideOutput;

#[derive(Clone, Copy)]
enum Scenario {
    Continuous,
    Reconnect,
}

impl Scenario {
    fn name(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Reconnect => "reconnect",
        }
    }
}

struct Publisher(Child);

impl Publisher {
    fn signal(&self, signal: &str) -> Result<()> {
        let status = Command::new("kill")
            .args([signal, &self.0.id().to_string()])
            .status()?;
        ensure!(status.success(), "publisher signal {signal} failed");

        Ok(())
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct MemoryRun {
    directory: PathBuf,
    fixture: PathBuf,
    url: String,
    config: OutputConfig,
    csv: fs::File,
    started: Instant,
    baseline: u64,
    budget: u64,
    max_retained: u64,
    warmup: usize,
    cycles: usize,
    seconds: usize,
}

fn setting(name: &str, default: usize) -> Result<usize> {
    match env::var(name) {
        Ok(value) => {
            let value = value.parse::<usize>().with_context(|| name.to_string())?;
            ensure!(value > 0, "{name} must be positive");

            Ok(value)
        }
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

impl MemoryRun {
    fn new(scenario: Scenario) -> Result<Self> {
        let width = setting("FFPLAYOUT_MEMORY_WIDTH", 1920)?;
        let height = setting("FFPLAYOUT_MEMORY_HEIGHT", 1080)?;
        ensure!(
            width.is_multiple_of(2) && height.is_multiple_of(2),
            "dimensions must be even"
        );
        let config = OutputConfig::new(width.try_into()?, height.try_into()?, 25, 48_000);
        crate::init_ffmpeg(&config)?;
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let url = format!("rtmp://127.0.0.1:{port}/live/memory");
        let root = env::var_os("FFPLAYOUT_MEMORY_REPORT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/live-memory")
            });
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = root.join(format!(
            "{}-{}-{stamp}",
            scenario.name(),
            std::process::id()
        ));
        fs::create_dir_all(&directory)?;
        let fixture = directory.join("publisher.flv");
        let status = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg(format!("testsrc2=size={width}x{height}:rate=25"))
            .args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-t",
                "4",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-pix_fmt",
                "yuv420p",
                "-b:v",
                "5000k",
                "-minrate",
                "5000k",
                "-maxrate",
                "5000k",
                "-bufsize",
                "10000k",
                "-g",
                "50",
                "-bf",
                "0",
                "-threads",
                "2",
                "-c:a",
                "aac",
                "-b:a",
                "160k",
                "-ac",
                "2",
                "-f",
                "flv",
            ])
            .arg(&fixture)
            .status()
            .context("ffmpeg executable with libx264/AAC is required")?;
        ensure!(status.success(), "failed to generate the RTMP fixture");
        let mut csv = fs::File::create(directory.join("memory.csv"))?;
        writeln!(
            csv,
            "elapsed_seconds,cycle,phase,rss_bytes,threads,readers,stuck_readers,session,video_frames,audio_frames"
        )?;
        eprintln!("Memory test artifacts: {}", directory.display());

        Ok(Self {
            directory,
            fixture,
            url,
            config,
            csv,
            started: Instant::now(),
            baseline: 0,
            budget: u64::try_from(setting("FFPLAYOUT_MEMORY_MAX_GROWTH_MB", 64)?)?
                .checked_mul(1024 * 1024)
                .context("memory budget overflow")?,
            max_retained: 0,
            warmup: setting("FFPLAYOUT_MEMORY_WARMUP", 3)?,
            cycles: setting("FFPLAYOUT_MEMORY_CYCLES", 8)?,
            seconds: setting("FFPLAYOUT_MEMORY_SESSION_SECONDS", 3)?,
        })
    }

    fn publisher(&self, speed: &str, cycle: usize) -> Result<Publisher> {
        let log = fs::File::create(self.directory.join(format!("publisher-{cycle}.log")))?;
        let child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "warning",
                "-readrate",
                speed,
                "-stream_loop",
                "-1",
                "-i",
            ])
            .arg(&self.fixture)
            .args(["-c", "copy", "-f", "flv"])
            .arg(&self.url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()?;

        Ok(Publisher(child))
    }

    fn sample(
        &mut self,
        cycle: usize,
        phase: &str,
        live: &LiveReceiver,
        output: &MeasuredOutput,
    ) -> Result<u64> {
        let ps = Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()?;
        ensure!(ps.status.success(), "ps RSS measurement failed");
        let rss = String::from_utf8(ps.stdout)?.trim().parse::<u64>()? * 1024;
        let threads = fs::read_dir("/proc/self/task").ok().map(Iterator::count);
        let (readers, stuck) = live_reader_counts(&(self.config.channel_id, self.url.clone()));
        writeln!(
            self.csv,
            "{:.3},{cycle},{phase},{rss},{},{readers},{stuck},{},{},{}",
            self.started.elapsed().as_secs_f64(),
            threads.map_or_else(String::new, |value| value.to_string()),
            live.session_id,
            output.video,
            output.audio
        )?;
        self.csv.flush()?;

        Ok(rss)
    }

    fn pump_until(
        &mut self,
        cycle: usize,
        phase: &str,
        live: &mut LiveReceiver,
        output: &mut MeasuredOutput,
        until: impl Fn(&LiveReceiver, &MeasuredOutput) -> bool,
    ) -> Result<()> {
        let deadline = Instant::now()
            + Duration::from_secs(30 + u64::try_from(self.seconds)? * 2)
            + LiveIdleTimeout::default().duration();
        let mut sample_at = Instant::now();
        let control = output.control.clone();

        loop {
            if let Err(error) = LiveOverrideOutput::new(output, live, &control).pump_live()
                && !error.is::<crate::playout::PlaybackSkipped>()
            {
                return Err(error);
            }

            if Instant::now() >= sample_at {
                self.sample(cycle, phase, live, output)?;
                sample_at = Instant::now() + Duration::from_millis(500);
            }

            if until(live, output) {
                return Ok(());
            }

            ensure!(
                Instant::now() < deadline,
                "timeout waiting for {phase}; see {}",
                self.directory.display()
            );
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn checkpoint(
        &mut self,
        cycle: usize,
        live: &mut LiveReceiver,
        output: &mut MeasuredOutput,
    ) -> Result<()> {
        let settle_until = Instant::now() + Duration::from_secs(1);
        self.pump_until(cycle, "settling", live, output, |_, _| {
            Instant::now() >= settle_until
        })?;
        let rss = self.sample(cycle, "checkpoint", live, output)?;

        if cycle == self.warmup {
            self.baseline = rss;
        } else if cycle > self.warmup {
            self.max_retained = self.max_retained.max(rss.saturating_sub(self.baseline));
        }

        Ok(())
    }

    fn finish(self) -> Result<()> {
        eprintln!(
            "RSS after warmup: {:.1} MiB; maximum checkpoint growth: {:.1} MiB; CSV: {}",
            self.baseline as f64 / 1048576.0,
            self.max_retained as f64 / 1048576.0,
            self.directory.join("memory.csv").display()
        );
        ensure!(
            self.max_retained <= self.budget,
            "resident memory grew by {} bytes after warmup (budget {}); inspect memory.csv and publisher logs",
            self.max_retained,
            self.budget
        );

        Ok(())
    }
}

// Keep the output encoder alive across all sessions, as in the reported setup.
// No frame history is retained by the instrumentation itself.
struct MeasuredOutput {
    inner: Output,
    control: PlaybackControl,
    video: usize,
    audio: usize,
}

impl FrameOutput for MeasuredOutput {
    fn audio_frame_size(&self) -> usize {
        self.inner.audio_frame_size()
    }

    fn encode_video(&mut self, frame: &ffmpeg_next::frame::Video) -> Result<()> {
        self.inner.encode_video(frame)?;
        self.video += 1;
        // Use the cooperative cancellation check to yield after each frame.
        // The test resumes pumping without resetting the session or encoder;
        // even a permanently full input queue must allow RSS sampling.
        self.control.skip_current();

        Ok(())
    }

    fn encode_audio(&mut self, frame: &ffmpeg_next::frame::Audio) -> Result<()> {
        self.inner.encode_audio(frame)?;
        self.audio += 1;
        self.control.skip_current();

        Ok(())
    }
}

fn run(scenario: Scenario) -> Result<()> {
    let _ = env_logger::try_init();
    let mut run = MemoryRun::new(scenario)?;
    let mut config = run.config.clone();
    config.stream_type = StreamType::Custom;
    config.stream_format = "mpegts".into();
    config
        .video_options
        .insert("preset".into(), "veryfast".into());
    config
        .video_options
        .insert("rate_control".into(), "cbr".into());
    config.video_options.insert("maxrate".into(), "5000".into());
    config.audio_bitrate = 160_000;
    let mut output = MeasuredOutput {
        inner: Output::open_stream("/dev/null", &config)?,
        control: PlaybackControl::default(),
        video: 0,
        audio: 0,
    };
    let mut live = spawn_rtmp_listener(run.url.clone(), run.config.clone());
    // FFmpeg's listener is created on a background thread. No TCP readiness
    // probe: that would consume the listener's single RTMP connection.
    thread::sleep(Duration::from_millis(250));
    let mut publisher = None;

    for cycle in 1..=run.warmup + run.cycles {
        if publisher.is_none() {
            let speed = if matches!(scenario, Scenario::Reconnect) && cycle.is_multiple_of(2) {
                "1.5"
            } else {
                "1"
            };
            publisher = Some(run.publisher(speed, cycle)?);
        }

        let previous_session = live.session_id;
        run.pump_until(cycle, "takeover", &mut live, &mut output, |live, _| {
            live.active
        })?;
        let session = live.session_id;

        if matches!(scenario, Scenario::Reconnect) {
            ensure!(
                session > previous_session,
                "publisher did not start a new session"
            );
        }

        let frames_before = output.video;
        let audio_before = output.audio;
        let target = frames_before + run.seconds * 25;
        run.pump_until(cycle, "streaming", &mut live, &mut output, |_, output| {
            output.video >= target
        })?;
        ensure!(
            output.audio > audio_before,
            "audio stopped during live playback"
        );

        if matches!(scenario, Scenario::Reconnect) {
            if cycle.is_multiple_of(2) {
                // Leave TCP connected but stop packet delivery. Resuming later
                // releases any reader blocked inside FFmpeg socket I/O.
                publisher
                    .as_ref()
                    .context("missing publisher")?
                    .signal("-STOP")?;
                let gap = LiveIdleTimeout::default().duration() + Duration::from_millis(500);
                run.pump_until(cycle, "packet_gap", &mut live, &mut output, |live, _| {
                    !live.active || live.last_media_at.is_some_and(|last| last.elapsed() >= gap)
                })?;
                publisher
                    .as_ref()
                    .context("missing publisher")?
                    .signal("-CONT")?;
                run.pump_until(cycle, "watchdog_end", &mut live, &mut output, |live, _| {
                    !live.active && !live.connecting
                })?;
            }

            drop(publisher.take());
            run.pump_until(cycle, "disconnected", &mut live, &mut output, |live, _| {
                !live.active && !live.connecting
            })?;
            ensure!(
                live.pending_audio.is_empty() && live.pending_audio_samples == 0,
                "pending audio retained after disconnect"
            );
            let cleanup_deadline = Instant::now() + Duration::from_secs(10);

            loop {
                let (readers, stuck) =
                    live_reader_counts(&(run.config.channel_id, run.url.clone()));

                // The next listening attempt itself holds one permit.
                if readers <= 1 && stuck == 0 {
                    break;
                }

                if Instant::now() >= cleanup_deadline {
                    bail!("reader cleanup did not finish: readers={readers}, stuck={stuck}");
                }
                thread::sleep(Duration::from_millis(50));
            }
        } else {
            ensure!(
                live.session_id == session && live.active,
                "continuous publisher was restarted"
            );
        }

        run.checkpoint(cycle, &mut live, &mut output)?;
    }

    drop(publisher);
    drop(live);
    output.inner.finish()?;
    run.finish()
}

#[test]
#[ignore = "isolated RTMP/RSS soak test; requires ffmpeg, ps, kill and local TCP"]
fn rtmp_memory_continuous_stream() -> Result<()> {
    run(Scenario::Continuous)
}

#[test]
#[ignore = "isolated RTMP/RSS soak test; requires ffmpeg, ps, kill and local TCP"]
fn rtmp_memory_reconnects() -> Result<()> {
    run(Scenario::Reconnect)
}
