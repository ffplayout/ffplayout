//! Destination reconnection keeps encoders and the program clock running.
use std::{
    collections::{BTreeMap, VecDeque},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use ffmpeg_next::{Rescale, codec, rescale};

use crate::utils::pipeline_queue::{PipelineControl, PipelineQueue};

use super::{
    encoded::EncodedOutput,
    encoding::EncodedPacket,
    mux::MuxOutput,
    parallel::{Message, OutputDestination, destination_mux},
    shared::Delivery,
};

enum DestinationOutput {
    Container(Box<EncodedOutput>),
    Native(MuxOutput),
}

impl DestinationOutput {
    fn open(
        destination: &OutputDestination,
        parameters: &[(codec::Parameters, codec::Parameters)],
    ) -> Result<Self> {
        if let Some(endpoint) = &destination.config.stream_endpoint {
            let streams = parameters
                .iter()
                .flat_map(|(video, audio)| [video, audio])
                .map(|parameters| {
                    // FFmpeg owns this initialization until parameters are dropped. Copy once
                    // for the connection; per-frame delivery never exposes FFmpeg pointers.
                    let initialization = unsafe {
                        let raw = &*parameters.as_ptr();
                        if raw.extradata.is_null() || raw.extradata_size <= 0 {
                            Vec::new()
                        } else {
                            std::slice::from_raw_parts(raw.extradata, raw.extradata_size as usize)
                                .to_vec()
                        }
                    };
                    crate::EncodedStream {
                        codec: parameters.id().name().into(),
                        initialization,
                    }
                })
                .collect::<Vec<_>>();
            let sink = endpoint.connect_packets(&streams)?;

            return Ok(Self::Native(MuxOutput::packets(
                sink,
                streams.len(),
                destination.config.channel_id,
            )?));
        }
        let (path, format) = destination_mux(destination)?;

        Ok(Self::Container(Box::new(EncodedOutput::open_packets(
            &path,
            &destination.config,
            format,
            parameters.to_vec(),
        )?)))
    }

    fn set_playout_rate(&mut self, rate: f64) {
        match self {
            Self::Container(output) => output.set_playout_rate(rate),
            Self::Native(output) => output.set_rate(rate),
        }
    }

    fn delivery_state(&self) -> (bool, Option<String>) {
        match self {
            Self::Container(output) => output.delivery_state(),
            Self::Native(output) => output.delivery_state(),
        }
    }

    fn observe_delivery(
        &self,
        activity: std::sync::Arc<crate::utils::pipeline_queue::PipelineActivity>,
    ) {
        match self {
            Self::Container(output) => output.observe_delivery(activity),
            Self::Native(output) => output.observe_delivery(activity),
        }
    }

    fn handle_encoded_packet(&mut self, mut packet: EncodedPacket) -> Result<()> {
        match self {
            Self::Container(output) => output.handle_encoded_packet(packet),
            Self::Native(output) => {
                packet.packet.set_stream(packet.stream);
                packet
                    .packet
                    .rescale_ts(packet.time_base, rescale::TIME_BASE);
                output.write(&mut packet.packet)
            }
        }
    }

    fn write_vtt_subtitles(&mut self, path: &str, start: i64, source: i64) -> Result<()> {
        match self {
            Self::Container(output) => output.write_vtt_subtitles(path, start, source),
            Self::Native(_) => Ok(()),
        }
    }

    fn advance_vtt_subtitles(&mut self, position: i64) -> Result<()> {
        match self {
            Self::Container(output) => output.advance_vtt_subtitles(position),
            Self::Native(_) => Ok(()),
        }
    }

    fn clear_vtt_subtitles(&mut self) {
        if let Self::Container(output) = self {
            output.clear_vtt_subtitles();
        }
    }

    fn finish(mut self) -> Result<()> {
        match self {
            Self::Container(output) => output.finish(),
            Self::Native(ref mut output) => output.finish(),
        }
    }
}

const RETRY_INTERVAL: Duration = Duration::from_secs(15);
const RETRY_WINDOW: Duration = Duration::from_secs(180);

struct Retry {
    interval: Duration,
    window: Duration,
    first_failure: Option<Instant>,
    next: Option<Instant>,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            interval: RETRY_INTERVAL,
            window: RETRY_WINDOW,
            first_failure: None,
            next: None,
        }
    }
}

impl Retry {
    fn reset(&mut self) {
        self.first_failure = None;
        self.next = None;
    }

    fn failed(&mut self, now: Instant) {
        self.first_failure.get_or_insert(now);
        self.next = Some(now + self.interval);
    }

    fn expired(&self, now: Instant) -> bool {
        self.first_failure
            .is_some_and(|first| now.saturating_duration_since(first) >= self.window)
    }

    fn due(&self, now: Instant) -> bool {
        !self.expired(now) && self.next.is_none_or(|next| now >= next)
    }

    fn phase(&self, now: Instant) -> &'static str {
        if self.expired(now) {
            "failed"
        } else {
            "retrying"
        }
    }
}

#[derive(Default)]
struct Keyframes {
    starts: BTreeMap<usize, i64>,
    pending_audio: VecDeque<EncodedPacket>,
    pending_audio_bytes: usize,
}

impl Keyframes {
    // Audio encoding can finish before the first video keyframe. Retain that
    // audio until its timestamp can be compared with the keyframe, rather than
    // making the beginning of an output depend on encoder thread scheduling.
    // Bound storage even when a rendition never produces a usable keyframe.
    const MAX_PENDING_PACKETS: usize = 512;
    const MAX_PENDING_BYTES: usize = 32 * 1024 * 1024;

    fn forward(
        &mut self,
        packet: EncodedPacket,
        mut emit: impl FnMut(EncodedPacket) -> Result<()>,
    ) -> Result<()> {
        let video = packet.stream & !1;

        if packet.stream != video && !self.starts.contains_key(&video) {
            let bytes = packet.packet.size();

            if bytes > Self::MAX_PENDING_BYTES {
                return Ok(());
            }

            while self.pending_audio.len() >= Self::MAX_PENDING_PACKETS
                || self.pending_audio_bytes > Self::MAX_PENDING_BYTES - bytes
            {
                if let Some(oldest) = self.pending_audio.pop_front() {
                    self.pending_audio_bytes -= oldest.packet.size();
                }
            }
            self.pending_audio_bytes += bytes;
            self.pending_audio.push_back(packet);

            return Ok(());
        }

        if !self.accept(&packet) {
            return Ok(());
        }

        let keyframe = packet.stream == video && packet.packet.is_key();
        emit(packet)?;

        if keyframe {
            for _ in 0..self.pending_audio.len() {
                let Some(pending) = self.pending_audio.pop_front() else {
                    break;
                };

                if pending.stream & !1 == video {
                    self.pending_audio_bytes -= pending.packet.size();

                    if self.accept(&pending) {
                        emit(pending)?;
                    }
                } else {
                    self.pending_audio.push_back(pending);
                }
            }
        }

        Ok(())
    }

    fn accept(&mut self, packet: &EncodedPacket) -> bool {
        let video = packet.stream & !1;
        let timestamp = packet
            .packet
            .dts()
            .or_else(|| packet.packet.pts())
            .map(|pts| pts.rescale(packet.time_base, rescale::TIME_BASE));
        if packet.stream == video && packet.packet.is_key() {
            self.starts
                .entry(video)
                .or_insert(timestamp.unwrap_or(i64::MIN));
        }

        self.starts
            .get(&video)
            .is_some_and(|start| timestamp.is_none_or(|pts| pts >= *start))
    }
}

pub(super) fn deliver(
    destination: OutputDestination,
    parameters: Vec<(codec::Parameters, codec::Parameters)>,
    queue: &PipelineQueue<Delivery>,
    control: &PipelineControl,
    interrupted: &AtomicBool,
) -> Result<()> {
    struct Disconnect(Option<std::sync::Arc<dyn super::parallel::OutputEndpoint>>);
    impl Drop for Disconnect {
        fn drop(&mut self) {
            if let Some(endpoint) = &self.0 {
                endpoint.disconnect();
            }
        }
    }
    let _disconnect = Disconnect(destination.config.stream_endpoint.clone());

    deliver_with_retry(
        destination,
        parameters,
        queue,
        control,
        interrupted,
        Retry::default(),
    )
}

fn deliver_with_retry(
    destination: OutputDestination,
    parameters: Vec<(codec::Parameters, codec::Parameters)>,
    queue: &PipelineQueue<Delivery>,
    control: &PipelineControl,
    interrupted: &AtomicBool,
    mut retry: Retry,
) -> Result<()> {
    let id = destination.id;
    let channel = destination.config.channel_id.unwrap_or_default();
    let status = &destination.config.output_status;
    let mut output: Option<DestinationOutput> = None;
    let mut keyframes = Keyframes::default();
    let mut error = None;
    let mut ready = false;
    let mut exhausted = false;
    let mut rate = 1.0;
    let mut subtitles: Option<(String, i64, i64)> = None;
    let mut subtitle_position = None;
    let activity = control.activity();
    status.observe_retry(id, &activity);

    loop {
        let observed = activity.snapshot();
        control.check()?;
        if queue.drained() {
            break;
        }

        if status.take_retry(id) && output.is_none() {
            retry.reset();
            exhausted = false;
        }

        if output.is_none() && retry.expired(Instant::now()) {
            if !exhausted {
                status.set(id, "failed", error.clone());
                exhausted = true;
            }
        } else if output.is_none() && retry.due(Instant::now()) && !queue.closed() {
            status.set(id, "starting", error.clone());
            let opened = (|| -> Result<DestinationOutput> {
                let mut opened = DestinationOutput::open(&destination, &parameters)?;
                opened.set_playout_rate(rate);
                if let Some((ref path, start, source)) = subtitles {
                    opened.write_vtt_subtitles(path, start, source)?;
                }
                if let Some(position) = subtitle_position {
                    opened.advance_vtt_subtitles(position)?;
                }
                Ok(opened)
            })();
            match opened {
                Ok(opened) => {
                    opened.observe_delivery(activity.clone());
                    // An open can block on I/O. Resume from current media rather than
                    // replaying the queue accumulated during the connection attempt.
                    if error.is_some() {
                        queue.discard_where(|message| matches!(message, Delivery::Packet(_)));
                    }
                    interrupted.store(false, Ordering::Release);
                    keyframes = Keyframes::default();
                    ready = false;
                    output = Some(opened);
                }
                Err(reason) => {
                    retry.failed(Instant::now());
                    error = Some(format!("{reason:#}"));
                    status.set(id, retry.phase(Instant::now()), error.clone());
                    log::warn!(channel = channel; "Output {id} connection failed: {reason:#}");
                }
            }
        }

        let (delivered, failure) = output
            .as_ref()
            .map(DestinationOutput::delivery_state)
            .unwrap_or_default();
        if delivered && !ready {
            ready = true;
            // Observe successful writes even if a later write has already failed.
            // Every successful connection gets a fresh budget for the next outage.
            retry.reset();
            exhausted = false;
            error = None;
            status.set(id, "running", None);
        }
        if let Some(reason) = failure {
            output = None;
            queue.discard_where(|message| matches!(message, Delivery::Packet(_)));
            retry.failed(Instant::now());
            error = Some(reason.clone());
            status.set(id, retry.phase(Instant::now()), error.clone());
            log::warn!(channel = channel; "Output {id} disconnected: {reason}");
        }

        let Some(message) = queue.try_recv()? else {
            if queue.closed() {
                break;
            }
            let now = Instant::now();
            let timeout = if output.is_none() && !retry.expired(now) {
                retry
                    .next
                    .into_iter()
                    .chain(retry.first_failure.map(|first| first + retry.window))
                    .min()
                    .map(|deadline| deadline.saturating_duration_since(now))
            } else {
                None
            };
            control.check()?;
            activity.wait(observed, timeout);
            continue;
        };
        let result = if interrupted.swap(false, Ordering::AcqRel) && output.is_some() {
            Err(anyhow!("destination queue exceeded its bounded capacity"))
        } else {
            match message {
                Delivery::Packet(packet) => {
                    if let Some(opened) = output.as_mut() {
                        keyframes.forward(packet, |packet| opened.handle_encoded_packet(packet))
                    } else {
                        Ok(())
                    }
                }
                Delivery::Control(message) => {
                    match message {
                        Message::Rate(value) => rate = value,
                        Message::Subtitles(ref path, start, source) => {
                            subtitles = Some((path.clone(), start, source));
                        }
                        Message::ClearSubtitles => {
                            subtitles = None;
                            subtitle_position = None;
                        }
                        Message::AdvanceSubtitles(position) => subtitle_position = Some(position),
                        Message::Video(..) | Message::Audio(..) | Message::Destinations(..) => {
                            unreachable!("delivery accepts compressed media")
                        }
                    }
                    if let Some(opened) = output.as_mut() {
                        match message {
                            Message::Rate(rate) => {
                                opened.set_playout_rate(rate);
                                Ok(())
                            }
                            Message::Subtitles(path, start, source) => {
                                opened.write_vtt_subtitles(&path, start, source)
                            }
                            Message::ClearSubtitles => {
                                opened.clear_vtt_subtitles();
                                Ok(())
                            }
                            Message::AdvanceSubtitles(position) => {
                                opened.advance_vtt_subtitles(position)
                            }
                            Message::Video(..) | Message::Audio(..) | Message::Destinations(..) => {
                                unreachable!()
                            }
                        }
                    } else {
                        Ok(())
                    }
                }
            }
        };
        if let Err(reason) = result {
            output = None;
            queue.discard_where(|message| matches!(message, Delivery::Packet(_)));
            retry.failed(Instant::now());
            error = Some(format!("{reason:#}"));
            status.set(id, retry.phase(Instant::now()), error.clone());
            log::warn!(channel = channel; "Output {id} disconnected: {reason:#}");
        }
    }

    if let Some(output) = output {
        output.finish()?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_packet_failures_reconnect_twice_without_opening_a_muxer() {
        use std::sync::{Arc, atomic::AtomicUsize};
        #[derive(Debug, Default)]
        struct Endpoint {
            connections: AtomicUsize,
            fail: Arc<AtomicBool>,
        }
        struct Sink(Arc<AtomicBool>);
        impl crate::PacketSink for Sink {
            fn write(&mut self, _: crate::EncodedFrame<'_>) -> Result<()> {
                if self.0.swap(false, Ordering::AcqRel) {
                    return Err(anyhow!("test native disconnect"));
                }
                Ok(())
            }
            fn finish(&mut self) -> Result<()> {
                Ok(())
            }
        }
        impl crate::OutputEndpoint for Endpoint {
            fn connect_packets(
                &self,
                streams: &[crate::EncodedStream],
            ) -> Result<Box<dyn crate::PacketSink>> {
                assert_eq!(streams.len(), 2);
                assert_eq!(streams[0].codec, "h264");
                assert_eq!(streams[1].codec, "aac");
                assert!(!streams[1].initialization.is_empty());
                self.connections.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(Sink(self.fail.clone())))
            }
        }
        let endpoint = Arc::new(Endpoint::default());
        endpoint.fail.store(true, Ordering::Release);
        let fixture = Fixture::configured(
            String::new(),
            Duration::from_millis(80),
            Duration::from_secs(2),
            |destination| {
                destination.config.stream_endpoint = Some(endpoint.clone());
            },
        );
        fixture.wait("starting");
        fixture.send_media();
        fixture.wait("retrying");
        fixture.wait("starting");
        fixture.send_media();
        fixture.wait("running");
        assert_eq!(endpoint.connections.load(Ordering::SeqCst), 2);
        endpoint.fail.store(true, Ordering::Release);
        fixture.send_media();
        fixture.wait("retrying");
        fixture.wait("starting");
        fixture.send_media();
        fixture.wait("running");
        assert_eq!(endpoint.connections.load(Ordering::SeqCst), 3);
        fixture.finish();
    }

    #[test]
    fn retries_are_spaced_and_stop_before_the_three_minute_deadline() {
        let start = Instant::now();
        let mut retry = Retry::default();
        retry.failed(start);
        for second in (15..180).step_by(15) {
            let now = start + Duration::from_secs(second);
            assert!(!retry.due(now - Duration::from_millis(1)));
            assert!(retry.due(now));
            retry.failed(now);
        }
        assert!(!retry.due(start + RETRY_WINDOW));
        assert!(!retry.due(start + RETRY_WINDOW + RETRY_INTERVAL));
    }

    #[test]
    fn successful_reconnection_resets_the_entire_failure_window() {
        let start = Instant::now();
        let mut retry = Retry::default();
        retry.failed(start);
        // A slow failing attempt must not move the original deadline.
        retry.failed(start + Duration::from_secs(170));
        assert!(retry.expired(start + RETRY_WINDOW));
        retry.reset();
        let later_failure = start + Duration::from_secs(500);
        retry.failed(later_failure);
        assert!(!retry.due(later_failure + Duration::from_secs(14)));
        assert!(retry.due(later_failure + RETRY_INTERVAL));
        assert!(!retry.expired(later_failure + Duration::from_secs(179)));
        assert!(retry.expired(later_failure + RETRY_WINDOW));
        retry.reset(); // The manual button starts a new round after exhaustion, too.
        assert!(retry.due(later_failure + RETRY_WINDOW));
    }

    struct Fixture {
        queue: std::sync::Arc<PipelineQueue<Delivery>>,
        control: std::sync::Arc<PipelineControl>,
        worker: Option<std::thread::JoinHandle<Result<()>>>,
        status: super::super::parallel::OutputStatus,
        packets: Vec<EncodedPacket>,
    }

    impl Fixture {
        fn new(url: String, interval: Duration, window: Duration) -> Self {
            Self::configured(url, interval, window, |_| {})
        }

        fn configured(
            url: String,
            interval: Duration,
            window: Duration,
            setup: impl FnOnce(&mut OutputDestination),
        ) -> Self {
            use super::super::encoding::EncodingProfile;
            use crate::utils::pipeline_queue::QueueLimits;
            use crate::{EncodingConfig, OutputConfig, StreamType};
            use ffmpeg_next::{
                ChannelLayout,
                format::{Pixel, Sample, sample::Type},
                frame,
            };
            ffmpeg_next::init().unwrap();
            let mut config = OutputConfig::new(160, 90, 25, 48_000);
            config.stream_type = StreamType::Custom;
            config.stream_format = "mpegts".into();
            config
                .video_options
                .insert("preset".into(), "ultrafast".into());
            config
                .video_options
                .insert("tune".into(), "zerolatency".into());
            let encode = EncodingConfig {
                width: config.width,
                height: config.height,
                video_codec: config.video_codec.clone(),
                video_options: config.video_options.clone(),
                audio_codec: config.audio_codec.clone(),
                audio_options: config.audio_options.clone(),
                audio_bitrate: config.audio_bitrate,
            };
            let (mut profile, video, audio) =
                EncodingProfile::open_shared(&config, &encode, 5).unwrap();
            let mut packets = Vec::new();
            for index in 0..10 {
                let mut video = frame::Video::new(Pixel::YUV420P, 160, 90);
                for plane in 0..video.planes() {
                    video.data_mut(plane).fill(128);
                }
                video.set_pts(Some(index));
                let prepared = crate::output::composition::ProgramVideo::new(
                    &video,
                    std::sync::Arc::new(std::sync::Mutex::new(
                        crate::output::composition::VideoCompositor::default(),
                    )),
                    None,
                    0.0,
                    BTreeMap::new(),
                )
                .unwrap();
                profile
                    .video(&prepared, |packet| {
                        packets.push(packet);
                        Ok(())
                    })
                    .unwrap();
                let mut audio =
                    frame::Audio::new(Sample::F32(Type::Planar), 1920, ChannelLayout::STEREO);
                audio.set_rate(48_000);
                audio.set_pts(Some(index * 1920));
                for channel in 0..2 {
                    audio.plane_mut::<f32>(channel).fill(0.1);
                }
                profile
                    .audio(&audio, |packet| {
                        packets.push(packet);
                        Ok(())
                    })
                    .unwrap();
            }
            profile
                .finish(|packet| {
                    packets.push(packet);
                    Ok(())
                })
                .unwrap();
            let status = config.output_status.clone();
            status.reconnectable(1, true);
            let control = std::sync::Arc::new(PipelineControl::default());
            let queue = PipelineQueue::new(
                QueueLimits {
                    items: 512,
                    bytes: 32 * 1024 * 1024,
                    duration_us: 1_000_000,
                },
                control.clone(),
            );
            let worker_queue = queue.clone();
            let worker_control = control.clone();
            let mut destination = OutputDestination {
                id: 1,
                config,
                target: super::super::parallel::OutputTarget::Stream { url },
            };
            setup(&mut destination);
            let worker = std::thread::spawn(move || {
                deliver_with_retry(
                    destination,
                    vec![(video, audio)],
                    &worker_queue,
                    &worker_control,
                    &AtomicBool::new(false),
                    Retry {
                        interval,
                        window,
                        ..Retry::default()
                    },
                )
            });
            Self {
                queue,
                control,
                worker: Some(worker),
                status,
                packets,
            }
        }

        fn wait(&self, phase: &str) {
            let deadline = Instant::now() + Duration::from_secs(3);
            while self
                .status
                .snapshot()
                .get(&1)
                .is_none_or(|state| state.phase != phase)
            {
                assert!(
                    Instant::now() < deadline,
                    "wanted {phase}, got {:?}",
                    self.status.snapshot()
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        fn send_media(&self) {
            for packet in &self.packets {
                let packet = EncodedPacket {
                    packet: crate::utils::ffmpeg::reference_packet(&packet.packet).unwrap(),
                    stream: packet.stream,
                    time_base: packet.time_base,
                };
                self.queue.send(Delivery::Packet(packet), 0, None).unwrap();
            }
        }

        fn finish(mut self) {
            self.queue.close();
            self.worker.take().unwrap().join().unwrap().unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.control.cancel();
            self.queue.close();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    #[test]
    fn endpoint_initialization_failure_is_retried_before_delivering_packets() {
        #[derive(Debug)]
        struct Endpoint {
            attempts: std::sync::atomic::AtomicUsize,
            packets: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }
        struct Sink(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl crate::PacketSink for Sink {
            fn write(&mut self, _: crate::EncodedFrame<'_>) -> Result<()> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }

            fn finish(&mut self) -> Result<()> {
                Ok(())
            }
        }
        impl crate::OutputEndpoint for Endpoint {
            fn connect_packets(
                &self,
                _: &[crate::EncodedStream],
            ) -> Result<Box<dyn crate::PacketSink>> {
                if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(anyhow!("publisher initialization failed"));
                }

                Ok(Box::new(Sink(self.packets.clone())))
            }
        }
        let endpoint = std::sync::Arc::new(Endpoint {
            attempts: std::sync::atomic::AtomicUsize::new(0),
            packets: Default::default(),
        });
        let fixture = Fixture::configured(
            String::new(),
            Duration::from_millis(100),
            Duration::from_secs(2),
            |destination| {
                destination.config.stream_endpoint = Some(endpoint.clone());
            },
        );
        fixture.wait("retrying");
        assert!(
            fixture.status.snapshot()[&1]
                .error
                .as_ref()
                .unwrap()
                .contains("publisher initialization")
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        while endpoint.attempts.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(30));
        fixture.send_media();
        fixture.wait("running");
        fixture.finish();
        assert!(endpoint.packets.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn subtitle_restore_failure_preserves_manual_recovery() {
        let directory = std::env::temp_dir().join(format!("subtitle-retry-{}", uuid_for_test()));
        std::fs::create_dir_all(&directory).unwrap();
        let media = directory.join("clip.mp4");
        std::fs::write(media.with_extension("vtt"), "invalid cue").unwrap();
        let fixture = Fixture::configured(
            String::new(),
            Duration::from_millis(30),
            Duration::from_millis(150),
            |destination| {
                destination.target = super::super::parallel::OutputTarget::Hls {
                    playlist: directory.join("index.m3u8").to_string_lossy().into_owned(),
                    variants: vec![],
                    subtitle: Some(crate::HlsSubtitle {
                        name: "Subtitles".into(),
                        language: "en".into(),
                        default: false,
                    }),
                    segment_seconds: 1,
                    list_size: 6,
                };
            },
        );
        fixture
            .queue
            .send(
                Delivery::Control(Message::Subtitles(
                    media.to_string_lossy().into_owned(),
                    0,
                    0,
                )),
                0,
                None,
            )
            .unwrap();
        fixture.wait("failed");
        assert!(
            !fixture.worker.as_ref().unwrap().is_finished(),
            "subtitle restoration killed the retry worker"
        );
        assert!(
            fixture.status.snapshot()[&1]
                .error
                .as_ref()
                .unwrap()
                .contains("VTT")
        );
        std::fs::write(
            media.with_extension("vtt"),
            "WEBVTT\n\n00:00:00.000 --> 00:00:00.300\nRecovered\n",
        )
        .unwrap();
        assert!(fixture.status.request_retry(1));
        fixture.wait("starting");
        std::thread::sleep(Duration::from_millis(80));
        fixture.send_media();
        fixture.wait("running");
        fixture.finish();
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_unavailable_tcp_destination_connects_when_the_server_appears() {
        use std::{io::Read, net::TcpListener, sync::mpsc, thread};
        let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let fixture = Fixture::new(
            format!("tcp://{address}"),
            Duration::from_millis(80),
            Duration::from_secs(2),
        );
        fixture.wait("retrying");
        let listener = TcpListener::bind(address).unwrap();
        let (accepted, connection) = mpsc::channel();
        let peer = thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "output did not reconnect");
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            accepted.send(()).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes).unwrap();
            bytes
        });
        connection.recv_timeout(Duration::from_secs(3)).unwrap();
        thread::sleep(Duration::from_millis(30));
        fixture.send_media();
        fixture.wait("running");
        assert!(fixture.status.snapshot()[&1].error.is_none());
        fixture.finish();
        let bytes = peer.join().unwrap();
        assert!(bytes.len() > 188);
        assert_eq!(bytes[0], 0x47);
    }

    #[test]
    fn a_midstream_tcp_disconnect_reconnects_without_reopening_the_encoder() {
        use std::{
            io::Read,
            net::{Shutdown, TcpListener},
            sync::mpsc,
            thread,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let (events, connection) = mpsc::channel();
        let peer = thread::spawn(move || {
            let mut recovered = Vec::new();
            for round in 0..2 {
                let deadline = Instant::now() + Duration::from_secs(4);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "output did not reconnect");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                events.send(round).unwrap();
                if round == 0 {
                    stream.read_exact(&mut [0_u8; 188]).unwrap();
                    stream.shutdown(Shutdown::Both).unwrap();
                } else {
                    stream.read_to_end(&mut recovered).unwrap();
                }
            }
            recovered
        });
        let fixture = Fixture::new(
            format!("tcp://{address}"),
            Duration::from_millis(80),
            Duration::from_secs(2),
        );
        assert_eq!(connection.recv_timeout(Duration::from_secs(3)).unwrap(), 0);
        thread::sleep(Duration::from_millis(30));
        fixture.send_media();
        fixture.wait("retrying");
        assert_eq!(connection.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
        thread::sleep(Duration::from_millis(30));
        // The same encoded packets/codec parameters are reused; no new EncodingProfile.
        fixture.send_media();
        fixture.wait("running");
        fixture.finish();
        let recovered = peer.join().unwrap();
        assert!(recovered.len() > 188);
        assert_eq!(recovered[0], 0x47);
    }

    #[test]
    fn exhausted_destination_stays_failed_until_manually_retried() {
        let directory = std::env::temp_dir().join(format!(
            "output-retry-{}-{}",
            std::process::id(),
            uuid_for_test()
        ));
        let path = directory.join("recovered.ts");
        let fixture = Fixture::new(
            path.to_string_lossy().into_owned(),
            Duration::from_millis(30),
            Duration::from_millis(100),
        );
        fixture.status.set(2, "running", None);
        fixture.wait("failed");
        std::fs::create_dir_all(&directory).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !path.exists(),
            "exhausted destination retried automatically"
        );
        assert!(fixture.status.request_retry(1));
        assert!(!fixture.status.request_retry(1));
        fixture.wait("starting");
        std::thread::sleep(Duration::from_millis(30));
        fixture.send_media();
        fixture.wait("running");
        assert_eq!(fixture.status.snapshot()[&2].phase, "running");
        fixture.finish();
        let mut input = ffmpeg_next::format::input(&path).unwrap();
        let first_video = input
            .packets()
            .find(|(stream, _)| stream.parameters().medium() == ffmpeg_next::media::Type::Video)
            .unwrap()
            .1;
        assert!(first_video.is_key());
        drop(input);
        std::fs::remove_dir_all(directory).unwrap();
    }

    fn uuid_for_test() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn audio_encoded_before_video_waits_for_its_renditions_keyframe() {
        let packet = |stream, timestamp, time_base, key| {
            let mut packet = ffmpeg_next::Packet::copy(&[1, 2, 3]);
            packet.set_dts(Some(timestamp));

            if key {
                packet.set_flags(ffmpeg_next::packet::Flags::KEY);
            }

            EncodedPacket {
                packet,
                stream,
                time_base,
            }
        };
        let mut gate = Keyframes::default();
        let mut emitted = Vec::new();
        let mut emit = |packet: EncodedPacket| {
            emitted.push((packet.stream, packet.packet.dts().unwrap()));

            Ok(())
        };

        // Codec priming predates the video start; subsequent audio belongs to
        // the program even when the video encoder takes longer to produce it.
        for timestamp in [-1024, 0, 1024, 2048, 3072] {
            gate.forward(packet(1, timestamp, (1, 48_000).into(), false), &mut emit)
                .unwrap();
        }
        gate.forward(packet(3, 4096, (1, 48_000).into(), false), &mut emit)
            .unwrap();
        gate.forward(packet(0, 0, (1, 25).into(), true), &mut emit)
            .unwrap();
        // A second rendition must wait for its own keyframe. Its audio is
        // earlier than that keyframe and must still be discarded on reconnect.
        gate.forward(packet(2, 3, (1, 25).into(), true), &mut emit)
            .unwrap();
        assert_eq!(
            emitted,
            [(0, 0), (1, 0), (1, 1024), (1, 2048), (1, 3072), (2, 3)]
        );
        assert!(gate.pending_audio.is_empty());
        assert_eq!(gate.pending_audio_bytes, 0);
    }

    #[test]
    fn audio_waiting_for_a_missing_keyframe_has_bounded_storage() {
        let mut gate = Keyframes::default();

        for timestamp in 0..Keyframes::MAX_PENDING_PACKETS + 5 {
            let mut packet = ffmpeg_next::Packet::copy(&[1; 16]);
            packet.set_dts(Some(timestamp as i64));
            gate.forward(
                EncodedPacket {
                    packet,
                    stream: 1,
                    time_base: (1, 48_000).into(),
                },
                |_| panic!("audio must wait for a video keyframe"),
            )
            .unwrap();
        }
        assert_eq!(gate.pending_audio.len(), Keyframes::MAX_PENDING_PACKETS);
        assert_eq!(gate.pending_audio.front().unwrap().packet.dts(), Some(5));
        assert_eq!(
            gate.pending_audio_bytes,
            Keyframes::MAX_PENDING_PACKETS * 16
        );

        let packet = ffmpeg_next::Packet::copy(&vec![0; Keyframes::MAX_PENDING_BYTES]);
        gate.forward(
            EncodedPacket {
                packet,
                stream: 1,
                time_base: (1, 48_000).into(),
            },
            |_| panic!("audio must wait for a video keyframe"),
        )
        .unwrap();
        assert_eq!(gate.pending_audio.len(), 1);
        assert_eq!(gate.pending_audio_bytes, Keyframes::MAX_PENDING_BYTES);
    }

    #[test]
    fn reconnect_waits_for_each_renditions_keyframe_and_discards_older_audio() {
        let mut gate = Keyframes::default();
        let packet = |stream, timestamp, key| {
            let mut packet = ffmpeg_next::Packet::empty();
            packet.set_dts(Some(timestamp));
            if key {
                packet.set_flags(ffmpeg_next::packet::Flags::KEY);
            }
            EncodedPacket {
                packet,
                stream,
                time_base: (1, 1000).into(),
            }
        };
        assert!(!gate.accept(&packet(0, 1000, false)));
        assert!(!gate.accept(&packet(1, 1000, false)));
        assert!(gate.accept(&packet(0, 1100, true)));
        assert!(!gate.accept(&packet(1, 1090, false)));
        assert!(gate.accept(&packet(1, 1120, false)));
        assert!(!gate.accept(&packet(2, 1120, false)));
        assert!(gate.accept(&packet(2, 1200, true)));
    }
}
