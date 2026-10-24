//! Publish encoded access units directly, without a container or loopback socket.
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{Context, Result, bail, ensure};
use hang::moq_net;
use moq_mux::{catalog, clock, import};
use tokio_util::sync::CancellationToken;

use super::publisher::{PublicationStatus, StatusSlot};

pub(super) struct Publisher {
    tracks: Vec<import::Track>,
    catalog: catalog::Producer<()>,
    _broadcast: moq_net::broadcast::Producer,
    anchor: clock::Anchor,
    lanes: Vec<clock::Lane>,
    offset: Arc<Mutex<Option<i64>>>,
    status: StatusSlot,
    stopped: CancellationToken,
    running: bool,
    reorder_us: u64,
}

impl Publisher {
    pub(super) fn new(
        mut broadcast: moq_net::broadcast::Producer,
        streams: &[ff_engine::EncodedStream],
        offset: Arc<Mutex<Option<i64>>>,
        status: StatusSlot,
        stopped: CancellationToken,
    ) -> Result<Self> {
        ensure!(streams.len() == 2, "MoQ requires one video/audio rendition");
        let config = catalog::Config::default()
            .with_max_age(super::relay::RETENTION)
            .with_catalog(catalog::hang::Catalog::<()>::default());
        let catalog = catalog::Producer::new(&mut broadcast, config)?;
        let anchor = clock::Anchor::new(catalog.clock());
        let video = &streams[0];
        let format = video_format(&video.codec, &video.initialization)?;
        let mut init = import::VideoInit::new(format, video.initialization.clone());
        init.hint.optimize_for_latency = Some(true);
        let video =
            import::Track::video(broadcast.reserve_track("0.video")?, catalog.reserve(), init)?;
        let audio = &streams[1];
        let format = match audio.codec.as_str() {
            "aac" => import::AudioFormat::Aac,
            "opus" => import::AudioFormat::Opus,
            "mp3" => import::AudioFormat::Mp3,
            codec => bail!("MoQ direct audio codec is unsupported: {codec}"),
        };
        let audio = import::Track::audio(
            broadcast.reserve_track("1.audio")?,
            catalog.reserve(),
            import::AudioInit::new(format, audio.initialization.clone()),
        )?;

        Ok(Self {
            tracks: vec![video, audio],
            catalog,
            _broadcast: broadcast,
            anchor,
            lanes: vec![clock::Lane::default(), clock::Lane::default()],
            offset,
            status,
            stopped,
            running: false,
            reorder_us: 0,
        })
    }
}

fn video_format(codec: &str, initialization: &[u8]) -> Result<import::VideoFormat> {
    Ok(match codec {
        "h264" if initialization.first() == Some(&1) => import::VideoFormat::Avc1,
        "h264" => import::VideoFormat::Avc3,
        "hevc" if initialization.first() == Some(&1) => import::VideoFormat::Hvc1,
        "hevc" => import::VideoFormat::Hev1,
        "av1" => import::VideoFormat::Av01,
        "vp8" => import::VideoFormat::Vp8,
        "vp9" => import::VideoFormat::Vp9,
        codec => bail!("MoQ direct video codec is unsupported: {codec}"),
    })
}

impl Publisher {
    fn publish(&mut self, frame: ff_engine::EncodedFrame<'_>) -> Result<()> {
        let track = self
            .tracks
            .get_mut(frame.stream)
            .context("invalid MoQ stream index")?;
        // A common bias accommodates negative encoder priming DTS without
        // changing track spacing. Anchor decode time, so reordered video PTS
        // cannot anchor audio before the beginning of the broadcast clock.
        let timestamp = translate(
            &mut self.anchor,
            &mut self.lanes[frame.stream],
            frame.pts_us,
            frame.dts_us,
        )?;
        let end = if frame.duration_us > 0 {
            let end = timestamp
                .as_micros()
                .checked_add(frame.duration_us as u128)
                .context("MoQ frame end overflow")?;
            Some(moq_net::Timestamp::from_micros(u64::try_from(end)?)?)
        } else {
            None
        };
        let reorder_us = frame.pts_us.saturating_sub(frame.dts_us).max(0) as u64;
        let update_reorder = frame.stream == 0 && reorder_us > self.reorder_us;
        // Publish initial decoder configuration and reorder requirements together.
        let reserved = update_reorder.then(|| self.catalog.reserve());
        track
            .decode(frame.data.to_vec(), Some(timestamp))
            .with_context(|| format!("publishing MoQ stream {} frame", frame.stream))?;
        if update_reorder {
            let reorder = std::time::Duration::from_micros(reorder_us);
            let mut update = self.catalog.modify()?;
            for video in update.video.renditions.values_mut() {
                video.jitter = Some(video.jitter.unwrap_or_default().max(reorder));
            }
            update.commit()?;
            self.reorder_us = reorder_us;
        }
        drop(reserved);
        if frame.stream == 1 {
            // Audio has no keyframes: bound each group explicitly so live joins
            // and retention never depend on an indefinitely open audio group.
            track.cut(end).context("closing MoQ audio frame group")?;
            let mapped = i64::try_from(timestamp.as_micros())?;
            *self.offset.lock().unwrap_or_else(PoisonError::into_inner) =
                mapped.checked_sub(frame.pts_us);
        }
        if let Some(end) = end {
            self.anchor.extend(end);
        }
        if !self.running {
            *self.status.lock().unwrap_or_else(PoisonError::into_inner) = PublicationStatus {
                phase: "running",
                error: None,
            };
            self.running = true;
        }

        Ok(())
    }
}

fn translate(
    anchor: &mut clock::Anchor,
    lane: &mut clock::Lane,
    pts_us: i64,
    dts_us: i64,
) -> Result<moq_net::Timestamp> {
    let source = u64::try_from(i128::from(dts_us) + (1_i128 << 60))?;
    let decoded_at = anchor.translate(lane, moq_net::Timestamp::from_micros(source)?)?;
    let pts = i128::try_from(decoded_at.as_micros())? + i128::from(pts_us) - i128::from(dts_us);

    Ok(moq_net::Timestamp::from_micros(u64::try_from(pts)?)?)
}

impl ff_engine::PacketSink for Publisher {
    fn write(&mut self, frame: ff_engine::EncodedFrame<'_>) -> Result<()> {
        let result = self.publish(frame);
        if let Err(error) = &result {
            *self.status.lock().unwrap_or_else(PoisonError::into_inner) = PublicationStatus {
                phase: "failed",
                error: Some(format!("{error:#}")),
            };
        }

        result
    }

    fn finish(&mut self) -> Result<()> {
        for track in &mut self.tracks {
            track.finish()?;
        }
        self.catalog.finish()?;
        self.stopped.cancel();

        Ok(())
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.stopped.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff_engine::PacketSink;

    fn streams() -> Vec<ff_engine::EncodedStream> {
        vec![
            ff_engine::EncodedStream {
                codec: "h264".into(),
                initialization: vec![],
            },
            ff_engine::EncodedStream {
                codec: "aac".into(),
                initialization: vec![0x11, 0x90],
            },
        ]
    }

    #[test]
    fn shared_decode_clock_preserves_negative_priming_and_b_frame_pts() {
        let mut anchor = clock::Anchor::new(clock::Clock::new());
        let mut video = clock::Lane::default();
        let mut audio = clock::Lane::default();
        let first = translate(&mut anchor, &mut video, 0, -80_000)
            .unwrap()
            .as_micros();
        let priming = translate(&mut anchor, &mut audio, -21_333, -21_333)
            .unwrap()
            .as_micros();
        let future = translate(&mut anchor, &mut video, 120_000, -40_000)
            .unwrap()
            .as_micros();
        let reordered = translate(&mut anchor, &mut video, 40_000, 0)
            .unwrap()
            .as_micros();
        assert_eq!(first - priming, 21_333);
        assert_eq!(future - first, 120_000);
        assert_eq!(reordered - first, 40_000);
    }

    #[tokio::test]
    async fn direct_audio_preserves_payload_pts_and_bounded_groups() {
        let broadcast = moq_net::broadcast::Producer::new(Default::default());
        let offset = Arc::new(Mutex::new(None));
        let status = Arc::new(Mutex::new(PublicationStatus {
            phase: "starting",
            error: None,
        }));
        let stopped = CancellationToken::new();
        let mut publisher = Publisher::new(
            broadcast.clone(),
            &streams(),
            offset.clone(),
            status.clone(),
            stopped.clone(),
        )
        .unwrap();
        let mut audio = broadcast
            .consume()
            .track("1.audio")
            .unwrap()
            .subscribe(None)
            .await
            .unwrap();
        let payload = [1, 2, 3, 4];
        let mut times = Vec::new();
        for pts in [-21_333, 0] {
            publisher
                .write(ff_engine::EncodedFrame {
                    stream: 1,
                    data: &payload,
                    pts_us: pts,
                    dts_us: pts,
                    duration_us: 21_333,
                })
                .unwrap();
            let mut group =
                tokio::time::timeout(std::time::Duration::from_secs(2), audio.recv_group())
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
            let frame = group.read_frame().await.unwrap().unwrap();
            let media = hang::container::Frame::decode(frame.payload).unwrap();
            assert_eq!(&media.payload[..], &payload);
            times.push(media.timestamp.as_micros());
            assert!(group.read_frame().await.unwrap().is_none());
        }
        assert_eq!(times[1] - times[0], 21_333);
        assert_eq!(offset.lock().unwrap().unwrap(), times[1] as i64);
        assert_eq!(status.lock().unwrap().phase, "running");
        publisher.finish().unwrap();
        assert!(stopped.is_cancelled());
    }

    #[test]
    fn invalid_packets_fail_and_drop_stops_auxiliary_work() {
        let broadcast = moq_net::broadcast::Producer::new(Default::default());
        let status = Arc::new(Mutex::new(PublicationStatus {
            phase: "starting",
            error: None,
        }));
        let stopped = CancellationToken::new();
        let mut publisher = Publisher::new(
            broadcast,
            &streams(),
            Default::default(),
            status.clone(),
            stopped.clone(),
        )
        .unwrap();
        assert!(
            publisher
                .write(ff_engine::EncodedFrame {
                    stream: 2,
                    data: &[1],
                    pts_us: 0,
                    dts_us: 0,
                    duration_us: 0
                })
                .is_err()
        );
        assert_eq!(status.lock().unwrap().phase, "failed");
        drop(publisher);
        assert!(stopped.is_cancelled());
    }
}
