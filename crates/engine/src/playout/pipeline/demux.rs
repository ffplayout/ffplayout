use std::sync::atomic::Ordering;

use anyhow::anyhow;
use ffmpeg_next::{Packet, Rational, Rescale, format, rescale};
use log::info;

use crate::{
    benchmark,
    input::diagnostics::LiveInputStage,
    utils::pipeline_queue::{POLL_INTERVAL, run_worker},
};

use super::{NO_TIMESTAMP, WorkerContext, read_rate::ReadRate};

fn is_rtmp_disconnect(label: &str, error: ffmpeg_next::Error) -> bool {
    // FFmpeg's RTMP reader reports EIO both for an orderly publisher stop and
    // some transport failures. Treat it as a disconnect, without claiming that
    // the peer closed cleanly. Other protocols and decoder errors stay errors.
    (label.starts_with("rtmp://") || label.starts_with("rtmps://"))
        && error
            == ffmpeg_next::Error::Other {
                errno: ffmpeg_next::error::EIO,
            }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    context: WorkerContext,
    mut input: format::context::Input,
    mut read_rate: ReadRate,
    video_index: Option<(usize, Rational)>,
    audio_index: Option<(usize, Rational)>,
    audio_end_us: Option<i64>,
    last_video_dts: Option<i64>,
) {
    let WorkerContext {
        cfg,
        label,
        initial,
        control,
        video_packets,
        audio_packets,
        benchmark,
        diagnostics,
        progress,
        ..
    } = context;

    if let Some(benchmark) = benchmark {
        benchmark::activate(benchmark.clone());
    }
    run_worker(control, || {
        loop {
            control.check()?;

            if video_packets.closed() && audio_packets.closed() {
                break;
            }

            let mut packet = Packet::empty();

            match packet.read(&mut input) {
                Ok(()) => {}
                Err(ffmpeg_next::Error::Eof) => break,
                Err(ffmpeg_next::Error::Other { errno }) if errno == ffmpeg_next::error::EAGAIN => {
                    control.wait(POLL_INTERVAL)?;
                    continue;
                }
                Err(error) if diagnostics.is_some() && is_rtmp_disconnect(label, error) => {
                    info!(channel = cfg.channel_id.unwrap_or_default();
                        "Live listener #{} (RTMP) input disconnected",
                        diagnostics.map_or(0, |diagnostics| diagnostics.listener_id)
                    );
                    // Drain the packets already received just as for EOF;
                    // cancelling the pipeline would discard the buffered tail.
                    break;
                }
                Err(error) => return Err(anyhow!("reading input {label}: {error}")),
            }

            if let Some(diagnostics) = diagnostics {
                diagnostics.set_stage(LiveInputStage::ReadingPacket);
                diagnostics.packet_received();
            }

            let (queue, track, time_base) =
                if video_index.is_some_and(|(index, _)| index == packet.stream()) {
                    (
                        video_packets,
                        0,
                        video_index.map(|(_, tb)| tb).unwrap_or(rescale::TIME_BASE),
                    )
                } else if audio_index.is_some_and(|(index, _)| index == packet.stream()) {
                    (
                        audio_packets,
                        1,
                        audio_index.map(|(_, tb)| tb).unwrap_or(rescale::TIME_BASE),
                    )
                } else {
                    continue;
                };

            if video_packets.closed() {
                read_rate.finish(0);
            }

            if audio_packets.closed() {
                read_rate.finish(1);
            }

            if video_packets.closed() && audio_packets.closed() {
                break;
            }

            if queue.closed() {
                continue;
            }

            let pts_us = packet
                .dts()
                .or_else(|| packet.pts())
                .map(|pts| pts.rescale(time_base, rescale::TIME_BASE));
            read_rate.update(track, pts_us, control)?;
            let bytes = packet.size();
            let reached_video_end = track == 0
                && last_video_dts
                    .zip(packet.dts())
                    .is_some_and(|(last, dts)| dts >= last);
            let reached_audio_end = track == 1
                && !initial.source_timestamp_mode
                && audio_end_us.zip(pts_us).is_some_and(|(end, pts)| {
                    pts.saturating_add(packet.duration().rescale(time_base, rescale::TIME_BASE))
                        >= end
                });

            if queue.would_block(bytes, pts_us) {
                progress
                    .demux_blocked_track
                    .store(track as i64, Ordering::Release);
                control.notify_activity();
            }

            let sent = queue.send(packet, bytes, pts_us);
            progress
                .demux_blocked_track
                .store(NO_TIMESTAMP, Ordering::Release);
            control.notify_activity();

            if let Err(error) = sent
                && !queue.closed()
            {
                return Err(error);
            }

            if reached_audio_end {
                audio_packets.close();
            }

            if reached_video_end {
                video_packets.close();
            }
        }

        Ok(())
    });
    video_packets.close();
    audio_packets.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtmp_eio_is_a_disconnect_but_other_protocols_and_errors_are_not() {
        let io = ffmpeg_next::Error::Other {
            errno: ffmpeg_next::error::EIO,
        };
        for url in ["rtmp://localhost/live/test", "rtmps://localhost/live/test"] {
            assert!(is_rtmp_disconnect(url, io));
            for error in [
                ffmpeg_next::Error::InvalidData,
                ffmpeg_next::Error::Other {
                    errno: ffmpeg_next::error::EINVAL,
                },
            ] {
                assert!(!is_rtmp_disconnect(url, error));
            }
        }
        for url in [
            "/tmp/clip.flv",
            "srt://localhost:9000",
            "http://localhost/live.flv",
        ] {
            assert!(!is_rtmp_disconnect(url, io));
        }
    }
}
