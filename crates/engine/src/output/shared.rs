//! Destination-free encoding shared by compatible outputs, with isolated mux workers.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, anyhow, ensure};
use ffmpeg_next::{Rescale, codec, rescale};

use crate::{
    EncodingConfig, OutputConfig,
    compositor::logo::blend_logo,
    utils::{
        ffmpeg::{make_video_frame_writable, reference_packet},
        pipeline_queue::{PipelineControl, PipelineQueue, QueueLimits},
    },
};

use super::{
    encoded::EncodedOutput,
    encoding::{EncodedPacket, EncodingProfile, video::hls_gop_size},
    parallel::{Message, OutputDestination, OutputStatus, OutputTarget},
    recording::RecordingMuxer,
};

/// Sources are resolved by the application; each group owns exactly one set of renditions.
pub(super) fn groups(destinations: &[OutputDestination]) -> Result<Vec<Vec<OutputDestination>>> {
    let mut groups = BTreeMap::<i32, Vec<OutputDestination>>::new();
    let mut ids = BTreeSet::new();
    for destination in destinations {
        ensure!(ids.insert(destination.id), "duplicate output ID");
        if destination.config.encoding_source.is_none() {
            groups.insert(destination.id, vec![destination.clone()]);
        }
    }
    for destination in destinations {
        if let Some((source, rendition)) = destination.config.encoding_source {
            let group = groups
                .get_mut(&source)
                .context("encoding source must be an owning output")?;
            let count = match &group[0].target {
                OutputTarget::Hls { variants, .. } => variants.len().max(1),
                _ => 1,
            };
            ensure!(
                rendition < count,
                "encoding source rendition is unavailable"
            );
            group.push(destination.clone());
        }
    }

    Ok(groups.into_values().collect())
}

pub(super) enum Delivery {
    Packet(EncodedPacket),
    Control(Message),
}

struct Sink {
    id: i32,
    queue: Arc<PipelineQueue<Delivery>>,
    control: Arc<PipelineControl>,
    status: OutputStatus,
    worker: Option<JoinHandle<()>>,
    interrupted: Arc<AtomicBool>,
}

impl Sink {
    fn start(
        destination: OutputDestination,
        parameters: Vec<(codec::Parameters, codec::Parameters)>,
    ) -> Result<Self> {
        let control = Arc::new(PipelineControl::default());
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
        let id = destination.id;
        let status = destination.config.output_status.clone();
        let worker_status = status.clone();
        let channel = destination.config.channel_id.unwrap_or_default();
        let interrupted = Arc::new(AtomicBool::new(false));
        let worker_interrupted = interrupted.clone();
        let worker = thread::Builder::new()
            .name(format!("playout-delivery-{id}"))
            .spawn(move || {
                worker_status.reconnectable(id, true);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    super::delivery::deliver(
                        destination,
                        parameters,
                        &worker_queue,
                        &worker_control,
                        &worker_interrupted,
                    )
                }))
                .unwrap_or_else(|_| Err(anyhow!("output delivery worker panicked")));
                worker_status.reconnectable(id, false);
                match result {
                    Ok(()) if !worker_control.cancelled() => {
                        let previous = worker_status.snapshot().remove(&id);
                        if let Some(previous) =
                            previous.filter(|state| matches!(state.phase, "failed" | "retrying"))
                        {
                            worker_status.set(id, "failed", previous.error);
                        } else {
                            worker_status.set(id, "stopped", None);
                        }
                    }
                    Ok(()) => {}
                    Err(error) if !worker_control.cancelled() => {
                        log::warn!(channel = channel; "Output {id} stopped: {error:#}");
                        worker_status.set(id, "failed", Some(format!("{error:#}")));
                        worker_control.fail(error);
                    }
                    Err(_) => {}
                }
            })?;

        Ok(Self {
            id,
            queue,
            control,
            status,
            worker: Some(worker),
            interrupted,
        })
    }

    fn send(&self, message: Delivery, bytes: usize, timestamp: Option<i64>) {
        if self.control.cancelled() {
            return;
        }
        match self.queue.try_send(message, bytes, timestamp) {
            Ok(None) => {}
            result => {
                // A blocked muxer must resume from a fresh keyframe, never a damaged GOP.
                self.interrupted.store(true, Ordering::Release);
                self.queue
                    .discard_where(|message| matches!(message, Delivery::Packet(_)));
                if let Ok(Some(message @ Delivery::Control(_))) = result {
                    let _ = self.queue.try_send(message, bytes, timestamp);
                }
            }
        }
    }

    fn finish(&mut self) {
        self.queue.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.control.cancel();
        self.status.reconnectable(self.id, false);
        self.finish();
    }
}

pub(super) fn run(
    mut destinations: Vec<OutputDestination>,
    program: &OutputConfig,
    queue: &PipelineQueue<Message>,
    control: &PipelineControl,
) -> Result<()> {
    control.check()?;
    let root = &destinations[0];
    let config = &root.config;
    let variants = match &root.target {
        OutputTarget::Hls { variants, .. } => variants.as_slice(),
        _ => &[],
    };
    let gop = destinations
        .iter()
        .map(|destination| match destination.target {
            OutputTarget::Hls {
                segment_seconds, ..
            } => hls_gop_size(config.fps, segment_seconds),
            _ => config.fps.saturating_mul(2),
        })
        .min()
        .context("encoding group is empty")?;
    let mut profiles = Vec::new();
    let mut parameters = Vec::new();
    for index in 0..variants.len().max(1) {
        let variant = variants.get(index);
        let mut video_options = config.video_options.clone();
        if let Some(variant) = variant {
            video_options.insert("maxrate".into(), (variant.video_bitrate / 1000).to_string());
        }
        let encode = EncodingConfig {
            width: variant.map_or(config.width, |variant| variant.width),
            height: variant.map_or(config.height, |variant| variant.height),
            video_codec: config.video_codec.clone(),
            video_options,
            audio_codec: config.audio_codec.clone(),
            audio_options: config.audio_options.clone(),
            audio_bitrate: variant.map_or(config.audio_bitrate, |variant| variant.audio_bitrate),
        };
        let (profile, video, audio) = EncodingProfile::open_shared(program, &encode, gop)?;
        profiles.push(profile);
        parameters.push((video, audio));
    }
    let mut recording = destinations.iter().filter_map(|destination| destination.config.recording.as_ref())
        .find(|recording| recording.encode.is_some())
        .and_then(|recording| match EncodedOutput::open_recording(program, recording) {
            Ok(output) => Some(output),
            Err(error) => {
                log::error!(channel = program.channel_id.unwrap_or_default(); "Recording disabled: {error:#}");
                None
            }
        });
    // Packet-copy recording belongs to the encoding group, not a network session.
    let mut copies = Vec::new();
    for destination in &destinations {
        if let Some(recording) = destination
            .config
            .recording
            .as_ref()
            .filter(|r| r.encode.is_none())
        {
            let index = destination
                .config
                .encoding_source
                .map_or(recording.video_stream_index, |(_, index)| index);
            let (video, audio) = parameters
                .get(index)
                .context("recording rendition is unavailable")?;
            match RecordingMuxer::open_parameters(
                recording,
                (video.clone(), program.video_time_base),
                (audio.clone(), program.audio_time_base),
            ) {
                Ok(mux) => copies.push((index, Some(mux))),
                Err(error) => {
                    log::error!(channel = program.channel_id.unwrap_or_default(); "Recording disabled: {error:#}");
                }
            }
        }
    }
    let mut sinks = Vec::new();
    for mut destination in destinations.drain(..) {
        destination.config.recording = None;
        let rendition = destination.config.encoding_source.map(|(_, index)| index);
        let codecs = rendition.map_or_else(
            || parameters.clone(),
            |index| vec![parameters[index].clone()],
        );
        sinks.push((Sink::start(destination, codecs)?, rendition));
    }
    let mut emit = |index: usize, packet: EncodedPacket| -> Result<()> {
        for (rendition, recording) in &mut copies {
            if *rendition != index {
                continue;
            }
            if let Some(mux) = recording {
                let result = if packet.stream == 0 {
                    mux.write_video(&packet.packet, packet.time_base)
                } else {
                    mux.write_audio(&packet.packet, packet.time_base)
                };
                if let Err(error) = result {
                    log::error!(channel = program.channel_id.unwrap_or_default(); "Recording disabled: {error:#}");
                    if let Some(mux) = recording.take() {
                        let _ = mux.finish();
                    }
                }
            }
        }
        let bytes = packet.packet.size();
        let timestamp = packet
            .packet
            .dts()
            .or_else(|| packet.packet.pts())
            .map(|pts| pts.rescale(packet.time_base, rescale::TIME_BASE));
        for (sink, rendition) in &sinks {
            if !sink.control.cancelled() && rendition.is_none_or(|rendition| rendition == index) {
                sink.send(
                    Delivery::Packet(EncodedPacket {
                        packet: reference_packet(&packet.packet)?,
                        stream: if rendition.is_some() {
                            packet.stream
                        } else {
                            index * 2 + packet.stream
                        },
                        time_base: packet.time_base,
                    }),
                    bytes,
                    timestamp,
                );
            }
        }
        Ok(())
    };
    while let Some(message) = queue.recv()? {
        control.check()?;
        match message {
            Message::Video(mut frame, logo, opacity) => {
                if let Some(logo) = logo {
                    make_video_frame_writable(&mut frame)?;
                    blend_logo(&mut frame, &logo, opacity);
                }
                if let Some(output) = &mut recording
                    && let Err(error) = output.encode_video(&frame)
                {
                    log::error!(channel = program.channel_id.unwrap_or_default(); "Recording disabled: {error:#}");
                    recording = None;
                }
                for (index, profile) in profiles.iter_mut().enumerate() {
                    profile.video(&frame, |packet| emit(index, packet))?;
                }
            }
            Message::Audio(frame) => {
                if let Some(output) = &mut recording
                    && let Err(error) = output.encode_audio(&frame)
                {
                    log::error!(channel = program.channel_id.unwrap_or_default(); "Recording disabled: {error:#}");
                    recording = None;
                }
                for (index, profile) in profiles.iter_mut().enumerate() {
                    profile.audio(&frame, |packet| emit(index, packet))?;
                }
            }
            Message::Rate(rate) => {
                for (sink, _) in &sinks {
                    sink.send(Delivery::Control(Message::Rate(rate)), 0, None);
                }
            }
            Message::Subtitles(path, start, source) => {
                for (sink, _) in &sinks {
                    sink.send(
                        Delivery::Control(Message::Subtitles(path.clone(), start, source)),
                        path.len(),
                        None,
                    );
                }
            }
            Message::ClearSubtitles => {
                for (sink, _) in &sinks {
                    sink.send(Delivery::Control(Message::ClearSubtitles), 0, None);
                }
            }
            Message::AdvanceSubtitles(position) => {
                for (sink, _) in &sinks {
                    sink.send(
                        Delivery::Control(Message::AdvanceSubtitles(position)),
                        0,
                        None,
                    );
                }
            }
        }
    }
    for (index, profile) in profiles.into_iter().enumerate() {
        profile.finish(|packet| emit(index, packet))?;
    }
    for (_, recording) in copies {
        if let Some(mux) = recording
            && let Err(error) = mux.finish()
        {
            log::error!(channel = program.channel_id.unwrap_or_default(); "Recording drain failed: {error:#}");
        }
    }
    if let Some(output) = recording
        && let Err(error) = output.finish()
    {
        log::error!(channel = program.channel_id.unwrap_or_default(); "Recording drain failed: {error:#}");
    }
    for (sink, _) in &sinks {
        sink.queue.close();
    }
    for (sink, _) in &mut sinks {
        sink.finish();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn destination(id: i32) -> OutputDestination {
        let mut config = OutputConfig::new(320, 240, 25, 48_000);
        config.stream_type = crate::StreamType::Custom;
        config.stream_format = "mpegts".into();
        OutputDestination {
            id,
            config,
            target: OutputTarget::Stream {
                url: format!("output-{id}.ts"),
            },
        }
    }

    #[test]
    fn own_encoding_stays_separate_and_explicit_sources_share() {
        let first = destination(1);
        let mut second = destination(2);
        assert_eq!(groups(&[first.clone(), second.clone()]).unwrap().len(), 2);
        second.config.encoding_source = Some((1, 0));
        second.config.width = 160; // A follower's stale own settings must not select another encoder.
        let groups = groups(&[second, first]).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0][0].id, 1);
    }

    #[test]
    fn rejects_missing_owners_and_invalid_renditions() {
        let first = destination(1);
        let mut second = destination(2);
        second.config.encoding_source = Some((9, 0));
        assert!(groups(&[first.clone(), second.clone()]).is_err());
        second.config.encoding_source = Some((1, 1));
        assert!(groups(&[first, second]).is_err());
    }

    #[test]
    fn a_blocked_shared_delivery_does_not_cancel_another_delivery() {
        let status = OutputStatus::default();
        let make_sink = |id| {
            let control = Arc::new(PipelineControl::default());
            Sink {
                id,
                queue: PipelineQueue::new(
                    QueueLimits {
                        items: 1,
                        bytes: 1024,
                        duration_us: 1_000_000,
                    },
                    control.clone(),
                ),
                control,
                status: status.clone(),
                worker: None,
                interrupted: Arc::new(AtomicBool::new(false)),
            }
        };
        let blocked = make_sink(1);
        let healthy = make_sink(2);
        blocked.send(Delivery::Control(Message::Rate(1.0)), 0, None);
        blocked.send(Delivery::Control(Message::Rate(1.0)), 0, None);
        healthy.send(Delivery::Control(Message::Rate(1.0)), 0, None);
        assert!(!blocked.control.cancelled());
        assert!(blocked.interrupted.load(Ordering::Acquire));
        assert!(!healthy.control.cancelled());
        assert!(healthy.queue.try_recv().unwrap().is_some());
        assert!(!healthy.interrupted.load(Ordering::Acquire));
    }
}
