use std::{
    sync::{Arc, Condvar, Mutex, OnceLock, PoisonError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{Packet, Rational, Rescale, format, rescale};

use crate::{
    HlsHealth,
    clock::{PlayoutClock, SharedPlayoutClock},
    utils::ffmpeg::reference_packet,
    utils::pipeline_queue::PipelineActivity,
};

// Packet storage is compressed, and independent of resolution. The time bound
// permits reading across the half-second A/V chunks found in local MP4 files.
const MAX_PACKETS: usize = 512;
const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_AHEAD_US: i64 = 1_000_000;
const IDLE_DRAIN: Duration = Duration::from_millis(20);

struct PendingPacket {
    packet: Packet,
    timestamp_us: i64,
    rate: f64,
    enqueued: Option<Instant>,
}

struct Queue {
    packets: Vec<PendingPacket>,
    bytes: usize,
    last_input: Instant,
    finished: bool,
    aborted: bool,
    error: Option<String>,
    delivered: bool,
    #[cfg(test)]
    emitted: Vec<(usize, i64, Instant)>,
    #[cfg(test)]
    first_write_delay: Duration,
}

struct Shared {
    pace_packets: bool,
    queue: Mutex<Queue>,
    changed: Condvar,
    activity: OnceLock<Arc<PipelineActivity>>,
}

/// Owns the single destination's mux context. Encoding hands immutable packet
/// references to a bounded queue; only this worker owns output pacing and I/O.
/// HLS drains available packets without per-packet waits; its clock still bounds
/// file reading. Recording retains its direct, unpaced sink.
pub(super) struct MuxOutput {
    direct: Option<format::context::Output>,
    direct_delivered: bool,
    time_bases: Vec<Rational>,
    #[cfg(test)]
    pub(super) stream_bitrates: Vec<i64>,
    shared: Option<Arc<Shared>>,
    worker: Option<JoinHandle<Result<()>>>,
    rate: f64,
    health: Option<HlsHealth>,
    clock: SharedPlayoutClock,
}

impl MuxOutput {
    pub(super) fn new(
        output: format::context::Output,
        threaded: bool,
        health: Option<HlsHealth>,
        channel: Option<i32>,
    ) -> Result<Self> {
        let time_bases = output.streams().map(|stream| stream.time_base()).collect();
        #[cfg(test)]
        let stream_bitrates = output
            .streams()
            .map(|stream| stream.parameters().bit_rate())
            .collect();
        let mut mux = Self {
            direct: Some(output),
            direct_delivered: false,
            time_bases,
            #[cfg(test)]
            stream_bitrates,
            shared: None,
            worker: None,
            rate: 1.0,
            health,
            clock: Arc::new(Mutex::new(PlayoutClock::new())),
        };

        if threaded {
            let shared = Arc::new(Shared {
                activity: OnceLock::new(),
                // The program/input clock already limits production. HLS files
                // may catch up immediately; network destinations stay paced.
                pace_packets: mux
                    .direct
                    .as_ref()
                    .context("mux context missing")?
                    .format()
                    .name()
                    != "hls",
                queue: Mutex::new(Queue {
                    packets: Vec::new(),
                    bytes: 0,
                    last_input: Instant::now(),
                    finished: false,
                    aborted: false,
                    error: None,
                    delivered: false,
                    #[cfg(test)]
                    emitted: Vec::new(),
                    #[cfg(test)]
                    first_write_delay: Duration::ZERO,
                }),
                changed: Condvar::new(),
            });
            let worker_shared = shared.clone();
            let output = mux.direct.take().context("mux context missing")?;
            let health = mux.health.clone();
            let active_streams = output
                .streams()
                .filter(|stream| {
                    matches!(
                        stream.parameters().medium(),
                        ffmpeg_next::media::Type::Video | ffmpeg_next::media::Type::Audio
                    )
                })
                .map(|stream| stream.index())
                .collect::<Vec<_>>();
            let clock = mux.clock.clone();
            mux.worker = Some(
                thread::Builder::new()
                    .name("playout-mux".into())
                    .spawn(move || {
                        let result = run(
                            output,
                            &worker_shared,
                            health,
                            channel,
                            clock,
                            active_streams,
                        );
                        let mut queue = worker_shared
                            .queue
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        if let Err(error) = &result {
                            queue.error = Some(format!("{error:#}"));
                        }
                        queue.finished = true;
                        worker_shared.changed.notify_all();
                        if let Some(activity) = worker_shared.activity.get() {
                            activity.notify();
                        }

                        result
                    })
                    .context("starting playout mux worker")?,
            );
            mux.shared = Some(shared);
        }

        Ok(mux)
    }

    pub(super) fn clock(&self) -> Option<SharedPlayoutClock> {
        self.shared.as_ref().map(|_| self.clock.clone())
    }

    pub(super) fn observe_delivery(&self, activity: Arc<PipelineActivity>) {
        if let Some(shared) = &self.shared {
            let _ = shared.activity.set(activity);
        }
    }

    pub(super) fn time_base(&self, index: usize) -> Result<Rational> {
        self.time_bases
            .get(index)
            .copied()
            .context("output stream missing")
    }

    pub(super) fn set_rate(&mut self, rate: f64) {
        self.rate = rate;
    }

    pub(super) fn write(&mut self, packet: &mut Packet) -> Result<()> {
        let Some(shared) = &self.shared else {
            packet.write_interleaved(self.direct.as_mut().context("mux context missing")?)?;
            self.direct_delivered = true;
            if let Some(health) = &self.health {
                health.mark_muxed();
            }

            return Ok(());
        };
        let time_base = self.time_base(packet.stream())?;
        let timestamp_us = packet
            .dts()
            .or_else(|| packet.pts())
            .unwrap_or(0)
            .rescale(time_base, rescale::TIME_BASE);
        let bytes = packet.size();
        if bytes > MAX_BYTES {
            return Err(anyhow!("encoded packet exceeds output queue byte limit"));
        }

        let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);

        loop {
            if let Some(error) = &queue.error {
                return Err(anyhow!("output mux worker failed: {error}"));
            }
            if queue.finished || queue.aborted {
                return Err(anyhow!("output mux worker stopped"));
            }
            let earliest = queue
                .packets
                .iter()
                .map(|pending| pending.timestamp_us)
                .min();
            let latest = queue
                .packets
                .iter()
                .map(|pending| pending.timestamp_us)
                .max();
            let full = queue.packets.len() >= MAX_PACKETS
                || (!queue.packets.is_empty() && queue.bytes.saturating_add(bytes) > MAX_BYTES)
                || earliest.zip(latest).is_some_and(|(first, last)| {
                    timestamp_us
                        .max(last)
                        .saturating_sub(timestamp_us.min(first))
                        > MAX_AHEAD_US
                });
            if !full {
                break;
            }
            queue = shared
                .changed
                .wait(queue)
                .unwrap_or_else(PoisonError::into_inner);
        }
        queue.packets.push(PendingPacket {
            packet: reference_packet(packet)?,
            timestamp_us,
            rate: self.rate,
            enqueued: (shared.pace_packets && log::log_enabled!(log::Level::Trace))
                .then(Instant::now),
        });
        queue.bytes += bytes;
        queue.last_input = Instant::now();
        shared.changed.notify_all();

        Ok(())
    }

    pub(super) fn delivery_state(&self) -> (bool, Option<String>) {
        let Some(shared) = &self.shared else {
            return (self.direct_delivered, None);
        };
        let queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);

        (queue.delivered, queue.error.clone())
    }

    pub(super) fn finish(&mut self) -> Result<()> {
        if let Some(shared) = &self.shared {
            shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .finished = true;
            shared.changed.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow!("output mux worker panicked"))??;
        }
        if let Some(output) = &mut self.direct {
            output.write_trailer()?;
        }

        Ok(())
    }
}

impl Drop for MuxOutput {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .aborted = true;
            shared.changed.notify_all();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run(
    mut output: format::context::Output,
    shared: &Shared,
    health: Option<HlsHealth>,
    channel: Option<i32>,
    clock: SharedPlayoutClock,
    active_streams: Vec<usize>,
) -> Result<()> {
    // HLS write timing is no longer collected. Retain the optional diagnostics
    // for paced network destinations without adding measurements to HLS.
    let mut timing = (shared.pace_packets && log::log_enabled!(log::Level::Trace))
        .then(|| MuxTiming::new(output.nb_streams() as usize, channel));

    loop {
        let pending = {
            let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if queue.aborted {
                    return Ok(());
                }
                if let Some((index, first)) = queue
                    .packets
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, p)| p.timestamp_us)
                {
                    // Every active track has supplied a packet at or after
                    // this DTS. No fixed media-duration hold is needed: the
                    // input governor already provides bounded read-ahead.
                    let ready = active_streams.iter().all(|stream| {
                        queue.packets.iter().any(|pending| {
                            pending.packet.stream() == *stream
                                && pending.timestamp_us >= first.timestamp_us
                        })
                    });
                    if queue.finished
                        || ready
                        || queue.last_input.elapsed() >= IDLE_DRAIN
                        || queue.packets.len() >= MAX_PACKETS
                        || queue.bytes >= MAX_BYTES
                    {
                        let pending = queue.packets.remove(index);
                        queue.bytes -= pending.packet.size();
                        shared.changed.notify_all();
                        break pending;
                    }
                } else if queue.finished {
                    output.write_trailer()?;

                    return Ok(());
                }
                let (next, _) = shared
                    .changed
                    .wait_timeout(queue, IDLE_DRAIN)
                    .unwrap_or_else(PoisonError::into_inner);
                queue = next;
            }
        };
        // Sparse subtitle cues are released by the program scheduler. Their
        // timestamps must not anchor or advance the audio/video pacing clock.
        let delay = if active_streams.contains(&pending.packet.stream()) {
            let mut clock = clock.lock().unwrap_or_else(PoisonError::into_inner);
            clock.set_rate(pending.rate);
            clock.delay_until(Some(pending.timestamp_us), rescale::TIME_BASE)
        } else {
            Duration::ZERO
        };
        let delay = if shared.pace_packets {
            delay
        } else {
            Duration::ZERO
        };
        let deadline = Instant::now() + delay;
        {
            let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
            while !queue.aborted && Instant::now() < deadline {
                let (next, _) = shared
                    .changed
                    .wait_timeout(queue, deadline.saturating_duration_since(Instant::now()))
                    .unwrap_or_else(PoisonError::into_inner);
                queue = next;
            }
            if queue.aborted {
                return Ok(());
            }
        }
        let packet = pending.packet;
        let stream_index = packet.stream();
        #[cfg(test)]
        {
            let delay = std::mem::take(&mut shared.queue.lock().unwrap().first_write_delay);
            thread::sleep(delay);
        }
        let started = timing.as_ref().map(|_| Instant::now());
        packet
            .write_interleaved(&mut output)
            .context("muxing queued output packet")?;
        if let (Some(timing), Some(started)) = (&mut timing, started) {
            timing.record(
                stream_index,
                pending.timestamp_us,
                pending.enqueued.unwrap_or(started),
                started,
            );
        }
        #[cfg(test)]
        shared
            .queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .emitted
            .push((stream_index, pending.timestamp_us, Instant::now()));
        let mut queue = shared.queue.lock().unwrap_or_else(PoisonError::into_inner);
        if !queue.delivered {
            queue.delivered = true;
            if let Some(activity) = shared.activity.get() {
                activity.notify();
            }
        }
        drop(queue);
        if let Some(health) = &health {
            health.mark_muxed();
        }
    }
}

struct MuxTiming {
    channel: i32,
    tracks: Vec<MuxTrack>,
    reported: Instant,
}

#[derive(Default)]
struct MuxTrack {
    anchor: Option<(Instant, i64)>,
    last: Option<Instant>,
    packets: usize,
    gap_max: Duration,
    residence_max: Duration,
    mux_max: Duration,
    drift_ms: f64,
}

impl MuxTiming {
    fn new(streams: usize, channel: Option<i32>) -> Self {
        Self {
            channel: channel.unwrap_or_default(),
            tracks: (0..streams).map(|_| MuxTrack::default()).collect(),
            reported: Instant::now(),
        }
    }

    fn record(&mut self, stream: usize, pts_us: i64, enqueued: Instant, started: Instant) {
        if !log::log_enabled!(log::Level::Trace) {
            return;
        }
        let now = Instant::now();
        let track = &mut self.tracks[stream];
        let (wall, media) = *track.anchor.get_or_insert((now, pts_us));
        track.drift_ms =
            now.duration_since(wall).as_secs_f64() * 1000.0 - (pts_us - media) as f64 / 1000.0;
        if let Some(last) = track.last {
            track.gap_max = track.gap_max.max(now.duration_since(last));
        }
        track.last = Some(now);
        track.packets += 1;
        track.residence_max = track.residence_max.max(started.duration_since(enqueued));
        track.mux_max = track.mux_max.max(now.duration_since(started));
        if self.reported.elapsed() < Duration::from_secs(5) {
            return;
        }
        for (index, track) in self.tracks.iter_mut().enumerate() {
            log::trace!(channel = self.channel;
                "[Output Timing] stage=mux track={index} packets={} gap_max_ms={:.3} drift_ms={:.3} residence_max_ms={:.3} mux_call_max_ms={:.3}",
                track.packets,
                track.gap_max.as_secs_f64() * 1000.0,
                track.drift_ms,
                track.residence_max.as_secs_f64() * 1000.0,
                track.mux_max.as_secs_f64() * 1000.0
            );
            track.packets = 0;
            track.gap_max = Duration::ZERO;
            track.residence_max = Duration::ZERO;
            track.mux_max = Duration::ZERO;
        }
        self.reported = now;
    }
}

#[cfg(test)]
mod tests {
    use ffmpeg_next::codec;

    use super::*;

    fn sink() -> MuxOutput {
        sink_with_subtitles(false)
    }

    fn sink_with_subtitles(subtitles: bool) -> MuxOutput {
        ffmpeg_next::init().unwrap();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_filler/filler_0.mp4");
        let input = format::input(&fixture).unwrap();
        let mut output =
            format::output_as(if cfg!(windows) { "NUL" } else { "/dev/null" }, "null").unwrap();
        for source in input.streams() {
            let mut stream = output.add_stream(codec::Id::None).unwrap();
            stream.set_parameters(source.parameters());
            stream.set_time_base(Rational(1, 1_000_000));
        }
        if subtitles {
            let mut stream = output.add_stream(codec::Id::WEBVTT).unwrap();
            let mut parameters = codec::Parameters::new();
            parameters.set_medium(ffmpeg_next::media::Type::Subtitle);
            parameters.set_id(codec::Id::WEBVTT);
            stream.set_parameters(parameters);
            stream.set_time_base(Rational(1, 1_000_000));
        }

        output.write_header().unwrap();

        MuxOutput::new(output, true, None, None).unwrap()
    }

    fn send(mux: &mut MuxOutput, stream: usize, timestamp_us: i64) {
        let mut packet = Packet::copy(&[0; 16]);
        packet.set_stream(stream);
        let timestamp = timestamp_us.rescale(rescale::TIME_BASE, mux.time_base(stream).unwrap());
        packet.set_pts(Some(timestamp));
        packet.set_dts(Some(timestamp));
        mux.write(&mut packet).unwrap();
    }

    #[test]
    fn a_subtitle_before_the_first_video_does_not_anchor_the_media_clock() {
        let mut mux = sink_with_subtitles(true);
        let shared = mux.shared.clone().unwrap();
        send(&mut mux, 2, -5_000_000);
        let deadline = Instant::now() + Duration::from_secs(1);

        while shared.queue.lock().unwrap().emitted.is_empty() {
            assert!(Instant::now() < deadline, "subtitle was not released");
            thread::sleep(Duration::from_millis(5));
        }

        assert!(mux.clock.lock().unwrap().media_time_us().is_none());
        let started = Instant::now();
        send(&mut mux, 0, 10_000_000);
        send(&mut mux, 1, 10_000_000);
        mux.finish().unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn hls_drains_available_packets_without_waiting_for_media_deadlines() {
        ffmpeg_next::init().unwrap();
        let directory = std::env::temp_dir().join(format!(
            "hls-catchup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests_assets/storage/media_filler/filler_0.mp4");
        let mut input = format::input(&fixture).unwrap();
        let mut output =
            super::super::hls::output_context(directory.join("index.m3u8").to_str().unwrap())
                .unwrap();
        for source in input.streams() {
            let mut stream = output.add_stream(codec::Id::None).unwrap();
            stream.set_parameters(source.parameters());
            stream.set_time_base(source.time_base());
        }
        output.write_header().unwrap();
        let mut packets = [None, None];
        for (stream, packet) in input.packets() {
            if stream.index() < 2 && packets[stream.index()].is_none() {
                packets[stream.index()] = Some(packet);
            }
            if packets.iter().all(Option::is_some) {
                break;
            }
        }
        let mut mux = MuxOutput::new(output, true, None, None).unwrap();
        assert!(!mux.shared.as_ref().unwrap().pace_packets);
        mux.shared
            .as_ref()
            .unwrap()
            .queue
            .lock()
            .unwrap()
            .first_write_delay = Duration::from_millis(300);
        let clock = mux.clock().unwrap();
        let started = Instant::now();
        // Simulate a backlog that already exists after a slow write. The HLS
        // worker must drain it rather than enforce another realtime schedule.
        for timestamp_us in [0, 10_000_000] {
            for (index, source) in packets.iter().enumerate() {
                let mut packet = reference_packet(source.as_ref().unwrap()).unwrap();
                packet.set_stream(index);
                let timestamp =
                    timestamp_us.rescale(rescale::TIME_BASE, mux.time_base(index).unwrap());
                packet.set_pts(Some(timestamp));
                packet.set_dts(Some(timestamp));
                mux.write(&mut packet).unwrap();
            }
        }
        mux.finish().unwrap();
        assert!(started.elapsed() >= Duration::from_millis(300));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(
            clock.lock().unwrap().media_time_us().is_some(),
            "input read governor still needs a continuous realtime clock"
        );
        assert!(directory.join("index.m3u8").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn half_second_av_chunks_are_emitted_evenly_in_dts_order() {
        let mut mux = sink();
        let shared = mux.shared.clone().unwrap();
        let start = Instant::now();
        for chunk in 0..10 {
            // Reproduce MP4 demux order: twelve video frames followed by audio.
            for frame in 0..12 {
                send(&mut mux, 0, chunk * 480_000 + frame * 40_000);
            }
            for frame in 0..24 {
                send(&mut mux, 1, chunk * 480_000 + frame * 20_000);
            }
        }
        mux.finish().unwrap();
        let queue = shared.queue.lock().unwrap();
        assert_eq!(queue.emitted.len(), 360);
        assert!(queue.emitted.windows(2).all(|pair| pair[0].1 <= pair[1].1));
        for stream in 0..2 {
            let times: Vec<_> = queue
                .emitted
                .iter()
                .filter(|event| event.0 == stream)
                .collect();
            let max_gap = times
                .windows(2)
                .map(|p| p[1].2.duration_since(p[0].2))
                .max()
                .unwrap();
            assert!(
                max_gap < Duration::from_millis(120),
                "stream {stream}: {max_gap:?}"
            );
        }
        assert!(start.elapsed() >= Duration::from_millis(4_700));
        assert!(start.elapsed() < Duration::from_millis(5_500));
    }

    #[test]
    fn repeated_short_clips_do_not_accumulate_opening_delays() {
        let mut config = crate::OutputConfig::new(160, 90, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "null".into();
        let mut playout = crate::Playout::open_stream(
            if cfg!(windows) { "NUL" } else { "/dev/null" },
            config,
            1.0,
        )
        .unwrap();
        let start = Instant::now();
        for _ in 0..30 {
            playout
                .play_with_timing_and_logo_fade(
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../../tests_assets/storage/media_filler/filler_0.mp4"
                    ),
                    None,
                    Some(0.2),
                    Default::default(),
                )
                .unwrap();
            // Opening/probing the next source must fit into the read-ahead
            // reserve rather than resetting the output clock on every clip.
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(playout.timeline.video_pts(), 150);
        playout.finish().unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(5_900), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(6_500), "{elapsed:?}");
    }

    #[test]
    fn finishing_short_input_drains_all_packets() {
        let mut mux = sink();
        let shared = mux.shared.clone().unwrap();
        send(&mut mux, 0, 0);
        send(&mut mux, 1, 0);
        send(&mut mux, 0, 40_000);
        mux.finish().unwrap();
        assert_eq!(shared.queue.lock().unwrap().emitted.len(), 3);
    }

    #[test]
    fn dropping_interrupts_a_far_future_deadline() {
        let mut mux = sink();
        let shared = mux.shared.clone().unwrap();
        send(&mut mux, 0, 0);
        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.queue.lock().unwrap().emitted.is_empty() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        send(&mut mux, 0, 60_000_000);
        thread::sleep(Duration::from_millis(50));
        let start = Instant::now();
        drop(mux);
        assert!(start.elapsed() < Duration::from_millis(200));
        assert_eq!(shared.queue.lock().unwrap().emitted.len(), 1);
    }
}
