use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result, anyhow};
use ffmpeg_next::{Packet, Rational, Rescale, frame, rescale};

use crate::{
    benchmark,
    utils::{
        ffmpeg::{reference_audio_frame, reference_video_frame},
        pipeline_queue::{
            PipelineActivity, PipelineControl, PipelineQueue, QueueLimits, run_worker,
        },
    },
};

use super::{AudioOutputStream, VideoOutputStream};

pub(in crate::output) struct EncodedPacket {
    pub(in crate::output) packet: Packet,
    pub(in crate::output) stream: usize,
    pub(in crate::output) time_base: Rational,
}

pub(in crate::output) enum EncodeFrame {
    Video(frame::Video),
    Audio(frame::Audio),
}

pub(in crate::output) struct EncodingWorkers {
    video: Arc<PipelineQueue<EncodeFrame>>,
    audio: Arc<PipelineQueue<EncodeFrame>>,
    packets: Arc<PipelineQueue<EncodedPacket>>,
    control: Arc<PipelineControl>,
    activity: Arc<PipelineActivity>,
    threads: Vec<JoinHandle<()>>,
    pub(in crate::output) audio_frame_size: usize,
    video_time_base: Rational,
    audio_time_base: Rational,
    #[cfg(test)]
    video_delay_ms: Arc<std::sync::atomic::AtomicU64>,
}

impl EncodingWorkers {
    pub(in crate::output) fn start(
        mut video_streams: Vec<VideoOutputStream>,
        mut audio_streams: Vec<AudioOutputStream>,
    ) -> Result<Self> {
        let audio_frame_size = audio_streams
            .first()
            .map_or(0, |stream| stream.encoder.frame_size() as usize);
        let video_time_base = video_streams
            .first()
            .map_or(Rational(1, 25), |stream| stream.encoder.time_base());
        let audio_time_base = audio_streams
            .first()
            .map_or(Rational(1, 48_000), |stream| stream.encoder.time_base());
        let control = Arc::new(PipelineControl::default());
        let activity = control.activity();
        let video = PipelineQueue::new(
            QueueLimits {
                items: 4,
                bytes: 64 * 1024 * 1024,
                duration_us: 200_000,
            },
            control.clone(),
        );
        let audio = PipelineQueue::new(
            QueueLimits {
                items: 24,
                bytes: 2 * 1024 * 1024,
                duration_us: 500_000,
            },
            control.clone(),
        );
        let packets = PipelineQueue::<EncodedPacket>::new(
            QueueLimits {
                items: 256,
                bytes: 32 * 1024 * 1024,
                duration_us: 2_000_000,
            },
            control.clone(),
        );
        let remaining = Arc::new(AtomicUsize::new(2));
        let benchmark = benchmark::current();
        #[cfg(test)]
        let video_delay_ms = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut workers = Self {
            video: video.clone(),
            audio: audio.clone(),
            packets: packets.clone(),
            control: control.clone(),
            activity,
            threads: Vec::new(),
            audio_frame_size,
            video_time_base,
            audio_time_base,
            #[cfg(test)]
            video_delay_ms: video_delay_ms.clone(),
        };

        for is_video in [true, false] {
            let frames = if is_video {
                video.clone()
            } else {
                audio.clone()
            };
            let packets = packets.clone();
            let control = control.clone();
            let remaining = remaining.clone();
            let benchmark = benchmark.clone();
            #[cfg(test)]
            let video_delay_ms = video_delay_ms.clone();
            let mut video_streams = if is_video {
                std::mem::take(&mut video_streams)
            } else {
                Vec::new()
            };
            let mut audio_streams = if is_video {
                Vec::new()
            } else {
                std::mem::take(&mut audio_streams)
            };
            workers.threads.push(
                thread::Builder::new()
                    .name(
                        if is_video {
                            "video-encode"
                        } else {
                            "audio-encode"
                        }
                        .into(),
                    )
                    .spawn(move || {
                        if let Some(benchmark) = benchmark {
                            benchmark::activate(benchmark);
                        }
                        run_worker(&control, || {
                            while let Some(frame) = frames.recv()? {
                                match frame {
                                    EncodeFrame::Video(frame) => {
                                        #[cfg(test)]
                                        thread::sleep(std::time::Duration::from_millis(
                                            video_delay_ms.load(Ordering::Relaxed),
                                        ));

                                        for stream in &mut video_streams {
                                            stream.submit(&frame)?;
                                            stream.drain(|packet, stream, time_base| {
                                                enqueue(&packets, packet, stream, time_base)
                                            })?;
                                        }
                                    }
                                    EncodeFrame::Audio(frame) => {
                                        for stream in &mut audio_streams {
                                            stream.submit(&frame)?;
                                            stream.drain(|packet, stream, time_base| {
                                                enqueue(&packets, packet, stream, time_base)
                                            })?;
                                        }
                                    }
                                }
                            }

                            for stream in &mut audio_streams {
                                stream.flush_resampler(|packet, stream, time_base| {
                                    enqueue(&packets, packet, stream, time_base)
                                })?;
                                stream.encoder.send_eof()?;
                                stream.drain(|packet, stream, time_base| {
                                    enqueue(&packets, packet, stream, time_base)
                                })?;
                            }

                            for stream in &mut video_streams {
                                stream.encoder.send_eof()?;
                                stream.drain(|packet, stream, time_base| {
                                    enqueue(&packets, packet, stream, time_base)
                                })?;
                            }

                            Ok(())
                        });
                        frames.close();

                        if remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                            packets.close();
                        }
                    })
                    .context("starting encoder worker")?,
            );
        }

        Ok(workers)
    }

    pub(in crate::output) fn submit(
        &mut self,
        frame: EncodeFrame,
        mut emit: impl FnMut(EncodedPacket) -> Result<()>,
    ) -> Result<()> {
        let (queue, bytes, timestamp_us) = match &frame {
            EncodeFrame::Video(frame) => (
                &self.video,
                (0..frame.planes())
                    .map(|plane| frame.data(plane).len())
                    .sum(),
                frame
                    .pts()
                    .map(|pts| pts.rescale(self.video_time_base, rescale::TIME_BASE)),
            ),
            EncodeFrame::Audio(frame) => (
                &self.audio,
                frame.samples().saturating_mul(8),
                frame
                    .pts()
                    .map(|pts| pts.rescale(self.audio_time_base, rescale::TIME_BASE)),
            ),
        };
        // Actual media-time limits are enforced on decoded input and compressed
        // output; encoder queues additionally have small frame/byte bounds.
        let mut pending = frame;

        loop {
            let observed = self.activity.snapshot();
            let Some(frame) = queue.try_send(pending, bytes, timestamp_us)? else {
                break;
            };
            pending = frame;

            while let Some(packet) = self.packets.try_recv()? {
                emit(packet)?;
            }

            self.control.check()?;
            self.activity.wait(observed, None);
        }

        while let Some(packet) = self.packets.try_recv()? {
            emit(packet)?;
        }

        Ok(())
    }

    pub(in crate::output) fn video_frame(frame: &frame::Video) -> Result<EncodeFrame> {
        Ok(EncodeFrame::Video(reference_video_frame(frame)?))
    }

    pub(in crate::output) fn audio_frame(frame: &frame::Audio) -> Result<EncodeFrame> {
        Ok(EncodeFrame::Audio(reference_audio_frame(frame)?))
    }

    pub(in crate::output) fn finish(
        &mut self,
        mut emit: impl FnMut(EncodedPacket) -> Result<()>,
    ) -> Result<()> {
        self.video.close();
        self.audio.close();

        while !self.packets.drained() {
            let observed = self.activity.snapshot();
            while let Some(packet) = self.packets.try_recv()? {
                emit(packet)?;
            }

            if !self.packets.drained() {
                self.control.check()?;
                self.activity.wait(observed, None);
            }
        }

        for thread in self.threads.drain(..) {
            thread
                .join()
                .map_err(|_| anyhow!("encoder worker panicked"))?;
        }
        self.control.check()
    }
}

impl Drop for EncodingWorkers {
    fn drop(&mut self) {
        self.control.cancel();

        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn enqueue(
    queue: &PipelineQueue<EncodedPacket>,
    packet: Packet,
    stream: usize,
    time_base: Rational,
) -> Result<()> {
    let bytes = packet.size();
    let timestamp = packet
        .dts()
        .or_else(|| packet.pts())
        .map(|pts| pts.rescale(time_base, rescale::TIME_BASE));

    queue.send(
        EncodedPacket {
            packet,
            stream,
            time_base,
        },
        bytes,
        timestamp,
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use ffmpeg_next::{
        ChannelLayout,
        format::{Pixel, Sample, sample::Type},
    };

    use super::super::{audio::open_audio_encoder, video::open_video_encoder};
    use super::*;

    #[test]
    fn delayed_video_encoder_does_not_block_audio_encoding() {
        ffmpeg_next::init().unwrap();
        let cfg = crate::OutputConfig::new(64, 36, 25, 48_000);
        let video = open_video_encoder(0, &cfg, false, None, (64, 36), Some(50), false).unwrap();
        let audio = open_audio_encoder(1, &cfg, false, None).unwrap();
        let mut workers = EncodingWorkers::start(vec![video], vec![audio]).unwrap();
        workers.video_delay_ms.store(300, Ordering::Relaxed);
        let mut video = frame::Video::new(Pixel::YUV420P, 64, 36);
        video.set_pts(Some(0));
        let mut packets = Vec::new();
        workers
            .submit(EncodingWorkers::video_frame(&video).unwrap(), |packet| {
                packets.push(packet);

                Ok(())
            })
            .unwrap();

        for index in 0..3 {
            let mut audio =
                frame::Audio::new(Sample::F32(Type::Planar), 1024, ChannelLayout::STEREO);
            audio.set_rate(48_000);
            audio.set_pts(Some(index * 1024));
            audio.plane_mut::<f32>(0).fill(0.0);
            audio.plane_mut::<f32>(1).fill(0.0);
            workers
                .submit(EncodingWorkers::audio_frame(&audio).unwrap(), |packet| {
                    packets.push(packet);

                    Ok(())
                })
                .unwrap();
        }

        let deadline = Instant::now() + Duration::from_millis(200);

        while !packets.iter().any(|packet| packet.stream == 1) && Instant::now() < deadline {
            if let Some(packet) = workers.packets.try_recv().unwrap() {
                packets.push(packet);
            }
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            packets.iter().any(|packet| packet.stream == 1),
            "audio should encode during video delay"
        );
        assert!(!packets.iter().any(|packet| packet.stream == 0));
        workers
            .finish(|packet| {
                packets.push(packet);

                Ok(())
            })
            .unwrap();
        assert_eq!(
            packets.iter().filter(|packet| packet.stream == 0).count(),
            1
        );
    }
}
