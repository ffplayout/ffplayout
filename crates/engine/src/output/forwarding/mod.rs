//! Compressed live ingest forwarding. Each destination owns its own muxer and
//! bounded queue. The input thread publishes packet references only.
use std::{
    collections::{BTreeMap, VecDeque},
    ffi::{c_int, c_void},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use ffmpeg_next::{Dictionary, Packet, Rational, Rescale, codec, ffi, format, rescale};
use serde::{Deserialize, Serialize};

mod group;
mod source;
mod transcode;

use crate::utils::{
    ffmpeg::reference_packet,
    helper::open_network_output_with_callback,
    pipeline_queue::{PipelineControl, PipelineQueue, QueueLimits},
};

use super::{
    delivery::Retry,
    parallel::{OutputDestination, OutputStatus, OutputTarget},
};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForwardMode {
    #[default]
    Auto,
    Copy,
    Transcode,
}

impl ForwardMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Copy => "copy",
            Self::Transcode => "transcode",
        }
    }
}

impl std::str::FromStr for ForwardMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "copy" => Ok(Self::Copy),
            "transcode" => Ok(Self::Transcode),
            _ => Err("invalid forwarding mode"),
        }
    }
}

const LIMITS: QueueLimits = QueueLimits {
    items: 256,
    bytes: 16 * 1024 * 1024,
    duration_us: i64::MAX,
};
const POLL: Duration = Duration::from_millis(50);
const MAX_RETIRED: usize = 16;
const KEYFRAME_AUDIO_PACKETS: usize = 128;
const KEYFRAME_AUDIO_BYTES: usize = 2 * 1024 * 1024;
const MUX_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub(super) struct Route {
    pub(super) id: i32,
    listener: i32,
    url: String,
    muxer: String,
    protocol: BTreeMap<String, String>,
    muxer_options: BTreeMap<String, String>,
    metadata: BTreeMap<String, String>,
    profile: transcode::Profile,
    stopped: AtomicBool,
}

impl Route {
    fn equivalent(&self, other: &Self) -> bool {
        self.listener == other.listener
            && self.url == other.url
            && self.muxer == other.muxer
            && self.protocol == other.protocol
            && self.muxer_options == other.muxer_options
            && self.metadata == other.metadata
            && self.profile == other.profile
    }
}

#[derive(Debug, Default)]
pub(super) struct Routes(BTreeMap<i32, Arc<Route>>);

impl Routes {
    pub(super) fn prepare(destinations: Vec<OutputDestination>) -> Result<Vec<Route>> {
        destinations
            .into_iter()
            .filter(|output| output.config.delivery_enabled)
            .map(|output| {
                let OutputTarget::Stream { url } = output.target else {
                    anyhow::bail!("ingest forwarding requires a network stream output");
                };
                let cfg = output.config;
                let profile = transcode::Profile::new(&cfg);
                ensure!(
                    matches!(
                        cfg.stream_type,
                        crate::StreamType::Rtmp | crate::StreamType::Srt | crate::StreamType::Udp
                    ),
                    "ingest forwarding supports RTMP, SRT and UDP stream outputs"
                );
                let muxer = cfg.stream_type.muxer(&cfg.stream_format).to_owned();

                crate::validate_output_protocol_options(
                    cfg.stream_type,
                    &url,
                    &cfg.protocol_options,
                )
                .map_err(anyhow::Error::msg)?;
                crate::validate_muxer_options(&muxer, &cfg.muxer_options)
                    .map_err(anyhow::Error::msg)?;
                crate::validate_output_metadata(&cfg.metadata_options)
                    .map_err(anyhow::Error::msg)?;

                Ok(Route {
                    id: output.id,
                    listener: cfg.forward_listener.context("missing ingest listener")?,
                    url,
                    muxer,
                    protocol: cfg.protocol_options,
                    muxer_options: cfg.muxer_options,
                    metadata: cfg.metadata_options,
                    profile,
                    stopped: AtomicBool::new(false),
                })
            })
            .collect()
    }

    pub(super) fn replace(&mut self, routes: Vec<Route>) -> Vec<(i32, &'static str)> {
        let mut old = std::mem::take(&mut self.0);
        let mut updates = Vec::new();

        for route in routes {
            let id = route.id;
            if let Some(current) = old.get(&id)
                && current.equivalent(&route)
                && let Some(current) = old.remove(&id)
            {
                self.0.insert(id, current);
            } else {
                self.0.insert(id, Arc::new(route));
                updates.push((id, "waiting_input"));
            }
        }

        for (id, route) in old {
            route.stopped.store(true, Ordering::Release);

            if !self.0.contains_key(&id) {
                updates.push((id, "stopped"));
            }
        }

        updates
    }

    pub(super) fn contains(&self, id: i32) -> bool {
        self.0.contains_key(&id)
    }

    pub(super) fn is_current(&self, route: &Arc<Route>) -> bool {
        self.0
            .get(&route.id)
            .is_some_and(|current| Arc::ptr_eq(current, route))
    }

    pub(super) fn has_listener(&self, listener: i32) -> bool {
        self.0.values().any(|route| route.listener == listener)
    }

    pub(super) fn for_listener(&self, listener: i32) -> Vec<Arc<Route>> {
        self.0
            .values()
            .filter(|route| route.listener == listener)
            .cloned()
            .collect()
    }
}

impl Drop for Routes {
    fn drop(&mut self) {
        for route in self.0.values() {
            route.stopped.store(true, Ordering::Release);
        }
    }
}

#[derive(Clone)]
struct Track {
    input: usize,
    parameters: codec::Parameters,
    time_base: Rational,
    video: bool,
}

impl Track {
    fn from_stream(stream: format::stream::Stream<'_>, video: bool) -> Self {
        Self {
            input: stream.index(),
            parameters: stream.parameters().clone(),
            time_base: stream.time_base(),
            video,
        }
    }
}

struct Mailbox {
    control: Arc<PipelineControl>,
    queue: Arc<PipelineQueue<Packet>>,
    overflow: AtomicBool,
    retry_exhausted: AtomicBool,
}

impl Mailbox {
    fn new() -> Arc<Self> {
        let control = Arc::new(PipelineControl::default());
        let queue = PipelineQueue::new(LIMITS, control.clone());

        Arc::new(Self {
            control,
            queue,
            overflow: AtomicBool::new(false),
            retry_exhausted: AtomicBool::new(false),
        })
    }

    fn send(&self, packet: &Packet) {
        if self.control.cancelled() || self.retry_exhausted.load(Ordering::Acquire) {
            return;
        }

        if let Ok(reference) = reference_packet(packet)
            && !matches!(
                self.queue.try_send(reference, packet.size(), None),
                Ok(None)
            )
        {
            self.reset();
        }
    }

    fn reset(&self) {
        self.queue.discard_where(|_| true);
        self.overflow.store(true, Ordering::Release);
        self.control.notify_activity();
    }
}

type TrackPublication = Arc<Mutex<Option<Result<Vec<Track>, String>>>>;

struct Worker {
    route: Arc<Route>,
    mailbox: Arc<Mailbox>,
    thread: JoinHandle<()>,
    group: Option<u64>,
}

impl Worker {
    fn start(
        route: Arc<Route>,
        mailbox: Arc<Mailbox>,
        publication: TrackPublication,
        reasons: String,
        group: Option<u64>,
        status: &OutputStatus,
    ) -> Result<Self> {
        let status = status
            .scope_forwarding(&route)
            .context("forwarding destination changed")?;
        let delivery_lock = status.delivery_lock(route.id);
        let thread = thread::Builder::new()
            .name(format!("ingest-forward-{}", route.id))
            .spawn({
                let route = route.clone();
                let mailbox = mailbox.clone();

                move || {
                    // Serialize replacements of the same destination. The old
                    // worker cancels its AVIO before the replacement can open.
                    let _delivery = delivery_lock.lock().unwrap_or_else(PoisonError::into_inner);
                    let stop = Stop {
                        route: &route,
                        control: &mailbox.control,
                    };

                    if stop.requested() {
                        return;
                    }

                    status.set(route.id, "waiting_keyframe", None);
                    let result = (|| {
                        let tracks = loop {
                            let tracks = publication
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .clone();

                            if let Some(tracks) = tracks {
                                break tracks.map_err(anyhow::Error::msg)?;
                            }

                            if stop.requested() {
                                return Ok(());
                            }

                            mailbox.control.wait(POLL)?;
                        };

                        status.reconnectable(route.id, true);
                        status.observe_retry(route.id, &mailbox.control.activity());

                        deliver(&stop, &mailbox, &tracks, &publication, &reasons, &status)
                    })();
                    let cancelled = stop.requested();
                    mailbox.control.cancel();
                    mailbox.queue.discard_where(|_| true);
                    status.reconnectable(route.id, false);

                    if !route.stopped.load(Ordering::Acquire) {
                        match result {
                            Err(error) if !cancelled => status.fail(route.id, format!("{error:#}")),
                            _ => status.set(route.id, "waiting_input", None),
                        }
                    }
                }
            })?;

        Ok(Self {
            route,
            mailbox,
            thread,
            group,
        })
    }

    fn stop(self) -> JoinHandle<()> {
        self.mailbox.control.cancel();

        self.thread
    }
}

/// One publisher generation. Packets and metadata from different connections
/// never share a queue or a muxer, even when the listener ID stays the same.
pub(crate) struct Session<'a> {
    listener: i32,
    status: &'a OutputStatus,
    workers: BTreeMap<i32, Worker>,
    pending: BTreeMap<i32, (Arc<Route>, OutputStatus)>,
    retired: Vec<JoinHandle<()>>,
    groups: BTreeMap<u64, group::Group>,
    next_group: u64,
    source: source::Source,
}

impl<'a> Session<'a> {
    pub(crate) fn new(listener: i32, status: &'a OutputStatus) -> Self {
        Self {
            listener,
            status,
            workers: BTreeMap::new(),
            pending: BTreeMap::new(),
            retired: Vec::new(),
            groups: BTreeMap::new(),
            next_group: 0,
            source: source::Source::default(),
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.workers.values().any(|worker| {
            !worker.route.stopped.load(Ordering::Acquire)
                && !worker.mailbox.control.cancelled()
                && !worker.mailbox.retry_exhausted.load(Ordering::Acquire)
        })
    }

    pub(crate) fn has_unseen_route(&self) -> bool {
        self.status.forwarding(self.listener).iter().any(|route| {
            !self
                .workers
                .get(&route.id)
                .is_some_and(|worker| Arc::ptr_eq(&worker.route, route))
                && !self
                    .pending
                    .get(&route.id)
                    .is_some_and(|(seen, _)| Arc::ptr_eq(seen, route))
        })
    }

    fn set_pending(&mut self, route: &Arc<Route>, phase: &'static str, error: Option<String>) {
        if let Some((_, scope)) = self.pending.get(&route.id) {
            scope.set(route.id, phase, error);
        } else if let Some(scope) = self.status.scope_forwarding(route) {
            scope.set(route.id, phase, error);
            self.pending.insert(route.id, (route.clone(), scope));
        }
    }

    pub(crate) fn publish(&mut self, input: &format::context::Input, packet: &Packet) -> bool {
        let routes = self.status.forwarding(self.listener);

        self.pending
            .retain(|_, (route, _)| !route.stopped.load(Ordering::Acquire));
        let mut index = self.retired.len();

        while index > 0 {
            index -= 1;

            if self.retired[index].is_finished() {
                let _ = self.retired.swap_remove(index).join();
            }
        }

        if routes.is_empty() && self.workers.is_empty() {
            self.source = source::Source::default();

            return false;
        }

        let source_changed = !routes.is_empty() && self.source.update(input, packet);
        let stopped: Vec<_> = self
            .workers
            .iter()
            .filter(|(_, worker)| source_changed || worker.route.stopped.load(Ordering::Acquire))
            .map(|(id, _)| *id)
            .collect();

        for id in stopped {
            if let Some(worker) = self.workers.remove(&id) {
                self.retired.push(worker.stop());
            }
        }

        if source_changed {
            self.retire_groups();
        }

        for route in routes {
            if self.retired.len() >= MAX_RETIRED
                || self.workers.contains_key(&route.id)
                || self.source.is_failed(&route)
            {
                continue;
            }

            if !self.source.ready() {
                self.set_pending(
                    &route,
                    "waiting_keyframe",
                    Some("waiting for ingest codec initialization".into()),
                );
                continue;
            }

            match self.start(route.clone(), self.source.tracks()) {
                Ok(worker) => {
                    self.pending.remove(&route.id);
                    self.workers.insert(route.id, worker);
                }
                Err(error) => {
                    // Keep failed compatibility decisions for this source
                    // generation without starting or retrying an encoder.
                    self.source.failed(route.id, route.clone());
                    self.set_pending(&route, "failed", Some(error.to_string()));
                }
            }
        }

        let unused: Vec<_> = self
            .groups
            .iter()
            .filter(|(id, _)| {
                !self
                    .workers
                    .values()
                    .any(|worker| worker.group == Some(**id) && !worker.thread.is_finished())
            })
            .map(|(id, _)| *id)
            .collect();

        for id in unused {
            if let Some(group) = self.groups.remove(&id) {
                self.retired.push(group.stop());
            }
        }

        if self.workers.is_empty() && !self.status.has_forwarding(self.listener) {
            self.source = source::Source::default();
        }

        if self.source.contains(packet.stream()) {
            for worker in self
                .workers
                .values()
                .filter(|worker| worker.group.is_none())
            {
                worker.mailbox.send(packet);
            }

            for group in self.groups.values() {
                group.send(packet);
            }
        }

        self.active()
    }

    fn start(&mut self, route: Arc<Route>, tracks: Vec<Track>) -> Result<Worker> {
        let plan = plan_tracks(&route, &tracks)?;
        let reasons = plan
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        let mailbox = Mailbox::new();

        if reasons.is_empty() {
            let publication = Arc::new(Mutex::new(Some(Ok(tracks))));

            return Worker::start(route, mailbox, publication, reasons, None, self.status);
        }

        let current = self
            .groups
            .iter()
            .find(|(_, group)| group.matches(&route, &plan) && group.try_add(&mailbox, &route));
        let (id, group) = if let Some((&id, group)) = current {
            (id, group)
        } else {
            let group = group::Group::start(&route, tracks, plan)?;
            let id = self.next_group;

            // A fresh group with no consumer or packets remains open for registration.
            ensure!(
                group.try_add(&mailbox, &route),
                "forwarding encoder stopped before registration"
            );
            self.next_group += 1;
            (id, &*self.groups.entry(id).or_insert(group))
        };
        Worker::start(
            route,
            mailbox,
            group.publication.clone(),
            reasons,
            Some(id),
            self.status,
        )
    }

    fn retire_groups(&mut self) {
        self.retired.extend(
            std::mem::take(&mut self.groups)
                .into_values()
                .map(group::Group::stop),
        );
    }
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        self.retire_groups();
        self.retired.extend(
            std::mem::take(&mut self.workers)
                .into_values()
                .map(Worker::stop),
        );
        for worker in self.retired.drain(..) {
            let _ = worker.join();
        }

        for (id, (_, scope)) in &self.pending {
            scope.set(*id, "waiting_input", None);
        }
    }
}

struct Stop<'a> {
    route: &'a Route,
    control: &'a PipelineControl,
}

impl Stop<'_> {
    fn requested(&self) -> bool {
        self.route.stopped.load(Ordering::Acquire) || self.control.cancelled()
    }
}

unsafe extern "C" fn interrupted(opaque: *mut c_void) -> c_int {
    // SAFETY: the destination thread retains Stop until after it drops AVIO.
    i32::from(unsafe { &*opaque.cast::<Stop<'_>>() }.requested())
}

struct Mux {
    output: format::context::Output,
    header_bytes: i64,
    start_us: i64,
    last_dts: Vec<Option<i64>>,
    pending_since: Option<Instant>,
    pending_packets: usize,
    pending_bytes: usize,
}

impl Mux {
    fn open(stop: &Stop<'_>, tracks: &[Track], start_us: i64) -> Result<Self> {
        let route = stop.route;
        let callback = ffi::AVIOInterruptCB {
            callback: Some(interrupted),
            opaque: std::ptr::from_ref(stop).cast_mut().cast(),
        };
        // SAFETY: only deliver calls this method. Its Stop outlives every Mux.
        let mut output = unsafe {
            open_network_output_with_callback(
                &route.url,
                Some(&route.muxer),
                &route.protocol,
                callback,
            )
        }
        .context("opening forwarding destination")?;

        for track in tracks {
            let mut stream = output.add_stream(codec::Id::None)?;

            stream.set_parameters(track.parameters.clone());
            stream.set_time_base(track.time_base);
            // SAFETY: the stream owns these copied parameters.
            unsafe { (*stream.parameters().as_mut_ptr()).codec_tag = 0 };
        }

        let mut metadata = Dictionary::new();
        let mut options = Dictionary::new();

        for (key, value) in &route.metadata {
            metadata.set(key, value);
        }

        for (key, value) in &route.muxer_options {
            options.set(key, value);
        }

        if route.muxer == "flv" && options.get("flvflags").is_none() {
            options.set("flvflags", "no_duration_filesize");
        }

        output.set_metadata(metadata);
        output
            .write_header_with(options)
            .context("writing forwarding header")?;
        let header_bytes = Self::flush_io(&mut output).context("flushing forwarding header")?;

        Ok(Self {
            output,
            header_bytes,
            start_us,
            last_dts: vec![None; tracks.len()],
            pending_since: None,
            pending_packets: 0,
            pending_bytes: 0,
        })
    }

    fn flush_io(output: &mut format::context::Output) -> Result<i64> {
        // SAFETY: an opened network output has an AVIO context owned by this worker.
        unsafe {
            let io = (*output.as_mut_ptr()).pb;

            ffi::avio_flush(io);

            if (*io).error < 0 {
                return Err(ffmpeg_next::Error::from((*io).error).into());
            }

            Ok((*io).bytes_written)
        }
    }

    fn has_written_media(&self) -> bool {
        // SAFETY: the output owns a live AVIO context. No other thread uses it.
        let io = unsafe { &*(*self.output.as_ptr()).pb };

        io.error >= 0 && io.bytes_written > self.header_bytes
    }

    fn write(&mut self, mut packet: Packet, track: &Track, index: usize) -> Result<()> {
        let time_base = self
            .output
            .stream(index)
            .context("missing forwarding stream")?
            .time_base();
        let offset = self.start_us.rescale(rescale::TIME_BASE, time_base);

        packet.rescale_ts(track.time_base, time_base);
        packet.set_pts(packet.pts().map(|pts| pts.saturating_sub(offset)));
        packet.set_dts(packet.dts().map(|dts| dts.saturating_sub(offset)));

        if let Some(dts) = packet.dts() {
            if self.last_dts[index].is_some_and(|last| dts <= last) {
                return Ok(());
            }

            self.last_dts[index] = Some(dts);
        }

        let bytes = packet.size();
        let now = Instant::now();

        self.flush_if_due(now, bytes, 1)?;
        packet.set_position(-1);
        packet.set_stream(index);
        packet
            .write_interleaved(&mut self.output)
            .context("writing forwarded packet")?;
        self.pending_since.get_or_insert(now);
        self.pending_packets += 1;
        self.pending_bytes += bytes;

        Ok(())
    }

    fn flush_if_due(&mut self, now: Instant, bytes: usize, packets: usize) -> Result<()> {
        if self.pending_since.is_some_and(|since| {
            now.saturating_duration_since(since) >= MUX_FLUSH_INTERVAL
                || self.pending_packets + packets > LIMITS.items
                || self.pending_bytes.saturating_add(bytes) > LIMITS.bytes
        }) {
            // FFmpeg can retain packets after a successful write while it
            // waits for another track. A null packet flushes that queue even
            // when max_interleave_delta disables its timestamp limit.
            // SAFETY: this thread owns the open output and all writes to it.
            let result = unsafe {
                ffi::av_interleaved_write_frame(self.output.as_mut_ptr(), std::ptr::null_mut())
            };

            if result < 0 {
                return Err(ffmpeg_next::Error::from(result)).context("flushing forwarded packets");
            }

            Self::flush_io(&mut self.output).context("flushing forwarded network data")?;
            self.pending_since = None;
            self.pending_packets = 0;
            self.pending_bytes = 0;
        }

        Ok(())
    }
}

fn copy_reason(route: &Route, track: &Track) -> Option<String> {
    let parameters = &track.parameters;
    let codec = parameters.id();
    // SAFETY: Track owns an immutable snapshot of the demuxer parameters.
    let raw = unsafe { &*parameters.as_ptr() };
    let compatible = match (route.muxer.as_str(), track.video) {
        ("mpegts", true) => matches!(
            codec,
            codec::Id::H264 | codec::Id::HEVC | codec::Id::MPEG2VIDEO
        ),
        ("mpegts", false) => matches!(
            codec,
            codec::Id::AAC | codec::Id::MP2 | codec::Id::MP3 | codec::Id::AC3
        ),
        ("flv", true) => codec == codec::Id::H264,
        ("flv", false) => matches!(codec, codec::Id::AAC | codec::Id::MP3),
        _ => false,
    };
    let kind = if track.video { "video" } else { "audio" };

    if !compatible {
        return Some(format!(
            "{kind} codec {} is incompatible with {}",
            codec.name(),
            route.muxer
        ));
    }

    if track.video {
        if raw.width <= 0 || raw.height <= 0 {
            return Some("video dimensions are unavailable".into());
        }

        if route.profile.resize && route.profile.dimensions != (raw.width as u32, raw.height as u32)
        {
            return Some("video resolution differs from the requested output size".into());
        }
    } else if raw.sample_rate <= 0
        || raw.ch_layout.nb_channels <= 0
        || (route.muxer == "flv" && raw.ch_layout.nb_channels > 2)
    {
        return Some("audio channel layout or sample rate is incompatible".into());
    }

    if route.muxer == "flv"
        && codec == codec::Id::MP3
        && !matches!(raw.sample_rate, 11_025 | 22_050 | 44_100)
    {
        return Some("MP3 sample rate is incompatible with FLV".into());
    }

    None
}

fn plan_tracks(route: &Route, tracks: &[Track]) -> Result<Vec<Option<String>>> {
    ensure!(!tracks.is_empty(), "ingest has no video or audio track");

    tracks
        .iter()
        .map(|track| {
            ensure!(
                track.time_base.numerator() > 0 && track.time_base.denominator() > 0,
                "invalid ingest stream time base"
            );
            let reason = copy_reason(route, track);

            match route.profile.mode {
                ForwardMode::Copy => {
                    if let Some(reason) = reason {
                        anyhow::bail!("{reason}");
                    }

                    Ok(None)
                }
                ForwardMode::Auto => Ok(reason),
                ForwardMode::Transcode => Ok(Some(
                    reason.unwrap_or_else(|| "transcode mode selected".into()),
                )),
            }
        })
        .collect()
}

fn deliver(
    stop: &Stop<'_>,
    mailbox: &Mailbox,
    tracks: &[Track],
    publication: &TrackPublication,
    reasons: &str,
    status: &OutputStatus,
) -> Result<()> {
    let queue = &mailbox.queue;
    let phase = if reasons.is_empty() {
        "copying"
    } else {
        "transcoding"
    };
    let id = stop.route.id;
    let video = tracks.iter().any(|track| track.video);
    let activity = stop.control.activity();
    let mut retry = Retry::default();
    let mut mux: Option<Mux> = None;
    let mut failure: Option<String> = None;
    let mut running = false;
    let mut audio: VecDeque<(Packet, usize, i64)> = VecDeque::new();
    let mut audio_bytes = 0;

    status.set(id, "waiting_keyframe", None);

    while !stop.requested() {
        let observed = activity.snapshot();
        let now = Instant::now();

        if status.take_retry(id) {
            retry.reset();
            failure = None;
            queue.discard_where(|_| true);
            audio.clear();
            audio_bytes = 0;
            mailbox.retry_exhausted.store(false, Ordering::Release);
        }

        if let Some(Err(error)) = &*publication.lock().unwrap_or_else(PoisonError::into_inner) {
            anyhow::bail!("{error}");
        }

        if mailbox.overflow.swap(false, Ordering::AcqRel) {
            mux = None;
            running = false;
            audio.clear();
            audio_bytes = 0;
            status.set(
                id,
                "waiting_keyframe",
                Some("forwarding resynchronizing".into()),
            );
        }

        if let Some(output) = &mut mux {
            if let Err(error) = output.flush_if_due(now, 0, 0) {
                failure = Some(format!("{error:#}"));
                retry.failed(Instant::now());
                mux = None;
                running = false;
                queue.discard_where(|_| true);
            } else if !running && output.has_written_media() {
                failure = None;
                retry.reset();
                status.set(id, phase, (!reasons.is_empty()).then(|| reasons.to_owned()));
                running = true;
            }
        }

        if let Some(reason) = &failure {
            mailbox
                .retry_exhausted
                .store(retry.expired(now), Ordering::Release);
            status.set(id, retry.phase(now), Some(reason.clone()));
        }

        let Some(packet) = queue.try_recv()? else {
            activity.wait(observed, Some(POLL));
            continue;
        };

        let Some((index, track)) = tracks
            .iter()
            .enumerate()
            .find(|(_, track)| track.input == packet.stream())
        else {
            continue;
        };
        let Some(timestamp) = packet.dts().or_else(|| packet.pts()) else {
            continue;
        };
        let timestamp_us = timestamp.rescale(track.time_base, rescale::TIME_BASE);

        if mux.is_none() {
            if !retry.due(now) {
                continue;
            }

            if video && !track.video {
                // The audio encoder can produce packets before the video
                // encoder emits its first keyframe. Keep the newest audio
                // so the start depends on media timestamps, not encoder delay.
                audio_bytes += packet.size();
                audio.push_back((packet, index, timestamp_us));

                while audio.len() > KEYFRAME_AUDIO_PACKETS || audio_bytes > KEYFRAME_AUDIO_BYTES {
                    if let Some((oldest, _, _)) = audio.pop_front() {
                        audio_bytes -= oldest.size();
                    }
                }

                continue;
            }

            if video && !packet.is_key() {
                continue;
            }

            match Mux::open(stop, tracks, timestamp_us) {
                Ok(output) => mux = Some(output),
                Err(error) => {
                    failure = Some(format!("{error:#}"));
                    retry.failed(Instant::now());
                    queue.discard_where(|_| true);
                    audio.clear();
                    audio_bytes = 0;
                    continue;
                }
            }
        }

        let Some(output) = &mut mux else { continue };

        if timestamp_us < output.start_us {
            continue;
        }

        let written = output.write(packet, track, index).and_then(|()| {
            audio_bytes = 0;

            for (packet, index, timestamp_us) in audio.drain(..) {
                if timestamp_us >= output.start_us {
                    output.write(packet, &tracks[index], index)?;
                }
            }

            Ok(())
        });

        if let Err(error) = written {
            failure = Some(format!("{error:#}"));
            retry.failed(Instant::now());
            mux = None;
            running = false;
            queue.discard_where(|_| true);
            audio.clear();
            audio_bytes = 0;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests;
