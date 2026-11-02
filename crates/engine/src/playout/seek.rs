//! Prime MPEG-TS decoders at a keyframe before the requested trim position.
use anyhow::{Context, Result, ensure};
use ffmpeg_next::{Packet, Rational, Rescale, format, media, rescale};

use crate::{PlaybackControl, utils::helper::open_media_input};

use super::{check_playback_control, seconds_to_microseconds, seek_input_at};

const MAX_PREROLL_PACKETS: usize = 4_096;
const MAX_PREROLL_BYTES: usize = 8 * 1024 * 1024;

pub(super) fn prepare(
    input: &mut format::context::Input,
    label: &str,
    seconds: f64,
    origin_us: i64,
    external_audio: bool,
    control: &PlaybackControl,
) -> Result<Vec<Packet>> {
    ensure!(
        seconds.is_finite() && seconds >= 0.0,
        "seek position must be a non-negative number"
    );

    if seconds == 0.0 {
        return Ok(Vec::new());
    }

    let video = input
        .streams()
        .best(media::Type::Video)
        .map(|stream| (stream.index(), stream.time_base()));

    let Some(video) = video.filter(|_| input.format().name() == "mpegts") else {
        seek_input_at(input, seconds, origin_us)?;
        return Ok(Vec::new());
    };
    let audio = (!external_audio)
        .then(|| {
            input
                .streams()
                .best(media::Type::Audio)
                .map(|stream| stream.index())
        })
        .flatten();
    let seek_us = seconds_to_microseconds(seconds);
    let target_us = origin_us.saturating_add(seek_us);
    let mut preroll_us = 1_000_000_i64;
    let mut repositioned = false;

    loop {
        check_playback_control(control)?;
        let earlier_us = seek_us.saturating_sub(preroll_us);

        if earlier_us <= 0 {
            // Seeking to the first presentation timestamp can itself skip an
            // earlier DTS/keyframe. Reopening preserves the original packet prefix.
            if repositioned {
                *input =
                    open_media_input(label).context("reopening TS input for decoder preroll")?;
            }

            return Ok(Vec::new());
        }

        repositioned = true;

        if seek_input_at(input, earlier_us as f64 / 1_000_000.0, origin_us).is_ok()
            && let Some(packets) = read_preroll(input, video, audio, target_us, control)?
        {
            return Ok(packets);
        }
        // A fixed small margin does not cover long GOPs. Widen until we find
        // a preceding keyframe, eventually falling back to the original prefix.
        preroll_us = preroll_us.saturating_mul(2).min(seek_us);
    }
}

fn read_preroll(
    input: &mut format::context::Input,
    (video_index, time_base): (usize, Rational),
    audio_index: Option<usize>,
    target_us: i64,
    control: &PlaybackControl,
) -> Result<Option<Vec<Packet>>> {
    let mut packets = Vec::new();
    let mut bytes = 0;

    for _ in 0..MAX_PREROLL_PACKETS {
        check_playback_control(control)?;
        let mut packet = Packet::empty();

        match packet.read(input) {
            Ok(()) => {}
            Err(ffmpeg_next::Error::Eof) => return Ok(None),
            Err(error) => return Err(error).context("reading TS decoder preroll"),
        }

        if packet.stream() == video_index {
            let timestamp = packet
                .pts()
                .or_else(|| packet.dts())
                .map(|pts| pts.rescale(time_base, rescale::TIME_BASE));

            if timestamp.is_none_or(|timestamp| timestamp > target_us) {
                return Ok(None);
            }

            if packet.is_key() && !packet.is_corrupt() {
                if bytes + packet.size() > MAX_PREROLL_BYTES {
                    return Ok(None);
                }

                packets.push(packet);
                return Ok(Some(packets));
            }
            // Never feed an undecodable partial GOP to the fresh decoder.
        } else if audio_index == Some(packet.stream()) {
            bytes += packet.size();

            if bytes > MAX_PREROLL_BYTES {
                return Ok(None);
            }

            packets.push(packet);
        }
    }

    Ok(None)
}

#[cfg(all(test, feature = "tokio"))]
mod tests {
    use std::{fs, path::Path, process::Command};

    use ffmpeg_next::media;

    use crate::{AsyncPlayout, ClipResult, LogoFade, OutputConfig};

    #[test]
    fn ts_seek_and_out_recover_without_fallback_or_extending_the_slot() {
        let directory =
            std::env::temp_dir().join(format!("ff-engine-ts-seek-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for name in ["with_audio", "short_audio"] {
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../tests_assets/storage/media_mix/{name}.mp4"));
            let ts = directory.join(format!("{name}.ts"));
            let remux = Command::new("ffmpeg")
                .args(["-nostdin", "-v", "error", "-y", "-i"])
                .arg(source)
                .args(["-map", "0:v:0", "-map", "0:a:0", "-c", "copy"])
                .arg(&ts)
                .output()
                .unwrap();
            assert!(
                remux.status.success(),
                "{}",
                String::from_utf8_lossy(&remux.stderr)
            );
            for (index, (seek, out)) in [
                (0.0, 1.0),
                (0.04, 1.04),
                (0.5, 1.5),
                (2.0, 3.0),
                (2.0, 4.5),
                (2.0, 2.12),
                (9.88, 10.0),
            ]
            .into_iter()
            .enumerate()
            {
                let path = directory.join(format!("{name}-{index}.ts"));
                runtime.block_on(async {
                    let mut config = OutputConfig::new(160, 90, 25, 48_000);
                    config
                        .video_options
                        .insert("preset".into(), "ultrafast".into());
                    config
                        .video_options
                        .insert("tune".into(), "zerolatency".into());
                    let output = AsyncPlayout::open_stream(path.to_string_lossy(), config, 1.0)
                        .await
                        .unwrap();
                    let result = output
                        .play_with_timing_and_logo_fade(
                            ts.to_string_lossy(),
                            Some(seek),
                            Some(out - seek),
                            None,
                            LogoFade::default(),
                        )
                        .await
                        .unwrap();
                    assert!(
                        matches!(result, ClipResult::Played),
                        "{name}, in={seek} out={out}: {result:?}"
                    );
                    output.finish().await.unwrap();
                });
                let mut input = ffmpeg_next::format::input(&path).unwrap();
                let video = input.streams().best(media::Type::Video).unwrap().index();
                let frames = input
                    .packets()
                    .filter(|(stream, _)| stream.index() == video)
                    .count();
                assert_eq!(
                    frames,
                    ((out - seek) * 25.0_f64).round() as usize,
                    "{name}, in={seek} out={out}"
                );
            }
        }
        fs::remove_dir_all(directory).unwrap();
    }
}
