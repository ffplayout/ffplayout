use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use log::*;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

use crate::{
    player::utils::{
        JsonPlaylist, Media, detect_audio_silence, is_close, is_remote, sec_to_time,
        time_in_seconds, time_to_sec,
    },
    utils::{config::PlayoutConfig, errors::ProcessError},
};

const MAX_AV_DURATION_DIFFERENCE_SECONDS: f64 = 4.0;

/// Validate a single media file.
///
/// - Check if file exists
/// - Check if the engine can probe the file
/// - Check if Metadata exists
async fn check_media(
    config: &PlayoutConfig,
    node: Media,
    pos: usize,
    begin: f64,
    cancel_token: CancellationToken,
) -> Result<(), ProcessError> {
    let id = config.general.channel_id;
    let mut error_list = vec![];

    if cancel_token.is_cancelled() {
        return Ok(());
    }

    if let Some(probe) = &node.probe {
        if probe.format.duration.is_none() && node.duration <= 0.0 {
            error_list.push("Engine probe returned no media duration".to_string());
        }

        if probe.video.is_empty() && probe.audio.is_empty() {
            error_list.push("Engine probe returned no audio or video stream".to_string());
        }
    } else {
        error_list.push("Engine probe returned no media metadata".to_string());
    }

    if !node.audio.is_empty() && node.probe_audio.is_none() {
        error_list.push(format!(
            "Engine probe returned no metadata for external audio: {}",
            node.audio
        ));
    }

    if let Some(error) = av_duration_error(&node) {
        error_list.push(error);
    }

    if config.logging.detect_silence
        && let Some(audio_source) = silence_check_source(&node)
    {
        match detect_audio_silence(audio_source, node.seek, node.out - node.seek).await {
            Ok(result) if result.silent => {
                error_list.push("Audio is totally silent!".to_string());
            }
            Ok(_) => {}
            Err(error) => {
                error!(channel = id;
                    "<span class=\"log-gray\">[Validation]</span> Silence detection failed on position <span class=\"log-number\">{pos}</span> - {}: <span class=\"log-addr\">{}</span>: {error}",
                    sec_to_time(begin),
                    node.source
                );
            }
        }
    }

    if !error_list.is_empty() {
        error!(channel = id;
            "<span class=\"log-gray\">[Validator]</span> Engine probe error on position <span class=\"log-number\">{pos}</span> - {}: <span class=\"log-addr\">{}</span>: {}",
            sec_to_time(begin),
            node.source,
            error_list.join("\n    ")
        );
    }

    Ok(())
}

fn av_duration_error(node: &Media) -> Option<String> {
    let probe = node.probe.as_ref()?;
    let video_duration = probe.video.first()?.duration?;
    let audio_duration = if node.audio.is_empty() {
        probe.audio.first()?.duration?
    } else {
        node.probe_audio.as_ref()?.audio.first()?.duration?
    };

    if !video_duration.is_finite() || !audio_duration.is_finite() {
        return None;
    }

    let difference = (video_duration - audio_duration).abs();
    if difference <= MAX_AV_DURATION_DIFFERENCE_SECONDS {
        return None;
    }

    Some(format!(
        "Audio/video stream durations differ by more than {} seconds. Video: {}, audio: {}, difference: {}",
        MAX_AV_DURATION_DIFFERENCE_SECONDS as u64,
        sec_to_time(video_duration),
        sec_to_time(audio_duration),
        sec_to_time(difference),
    ))
}

fn silence_check_source(node: &Media) -> Option<&str> {
    if !node.audio.is_empty() && node.probe_audio.is_some() {
        Some(&node.audio)
    } else if node
        .probe
        .as_ref()
        .is_some_and(|probe| !probe.audio.is_empty())
    {
        Some(&node.source)
    } else {
        None
    }
}

/// Validate Webvtt.
///
/// - Check if duration matches with video duration
async fn check_vtt(source: &str, duration: f64, channel_id: i32) -> Result<(), ProcessError> {
    let vtt_path = Path::new(source).with_extension("vtt");

    if vtt_path.is_file() {
        let file = File::open(&vtt_path).await?;
        let reader = BufReader::new(file);

        let mut last_timestamp = None;
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            if let Some((_start, end)) = line.split_once(" --> ") {
                last_timestamp = Some(end.trim().to_string());
            }
        }

        if let Some(time) = last_timestamp {
            let timestamp = if time.chars().filter(|&c| c == ':').count() == 1 {
                format!("00:{time}")
            } else {
                time
            };

            let last_sec = time_to_sec(&timestamp, &None);

            if last_sec > duration {
                warn!(channel = channel_id;
                    "<span class=\"log-gray\">[Validation]</span> Webvtt <span class=\"log-addr\">{vtt_path:?}</span> is longer, <span class=\"log-number\">{timestamp}</span> versus <span class=\"log-number\">{}</span> video duration.",
                    sec_to_time(duration)
                );
            }
        }
    }

    Ok(())
}

/// Validate a given playlist, to check if:
///
/// - the source files are existing
/// - file can be read by the engine probe and metadata exists
/// - total playtime fits target length from config
///
/// This function we run in a thread, to don't block the main function.
pub async fn validate_playlist(
    config: PlayoutConfig,
    current_list: Arc<Mutex<Vec<Media>>>,
    mut playlist: JsonPlaylist,
    is_alive: Arc<AtomicBool>,
    cancel_token: CancellationToken,
) {
    let id = config.general.channel_id;
    let date = playlist.date;

    let mut length = config.playlist.length_sec.unwrap();
    let mut begin = config.playlist.start_sec.unwrap();
    let mut time_sec = time_in_seconds(&config.channel.timezone);

    length += begin;

    if time_sec < config.playlist.start_sec.unwrap_or_default() {
        time_sec += 86400.0;
    }

    debug!(channel = id; "Validate playlist from: <span class=\"log-number\">{date}</span>");
    let timer = Instant::now();

    for (index, item) in playlist.program.iter_mut().enumerate() {
        if cancel_token.is_cancelled() || !is_alive.load(Ordering::SeqCst) {
            return;
        }

        let pos = index + 1;

        if begin < time_sec {
            // Do not validate clips that are being passed.
            begin += item.out - item.seek;
            continue;
        }

        if !is_remote(&item.source) {
            for path in [&mut item.source, &mut item.audio] {
                if !path.is_empty() && !is_remote(path) && Path::new(path).is_relative() {
                    let new_path = config.storage.path.join(&*path);
                    *path = new_path.to_string_lossy().to_string();
                }
            }

            if item.audio.is_empty() {
                if let Err(e) = item.add_probe(false).await {
                    error!(channel = id;
                        "<span class=\"log-gray\">[Validation]</span> Error on position <span class=\"log-number\">{pos:0>3}</span> - <span class=\"log-number\">{}</span>: {e}",
                        sec_to_time(begin)
                    );
                }
            } else if let Err(e) = item.add_probe(true).await {
                error!(channel = id;
                    "<span class=\"log-gray\">[Validation]</span> Error on position <span class=\"log-number\">{pos:0>3}</span> - <span class=\"log-number\">{}</span>: {e}",
                    sec_to_time(begin)
                );
            }
        }

        if item.probe.is_some() {
            match check_media(&config, item.clone(), pos, begin, cancel_token.clone()).await {
                Err(e) => {
                    error!(channel = id; "{e}");
                }
                Ok(()) => {
                    if config.general.validate {
                        debug!(
                            channel = id;
                            "<span class=\"log-gray\">[Validation]</span> Source at <span class=\"log-number\">{}</span>, seems fine: <span class=\"log-addr\">{}</span>",
                            sec_to_time(begin),
                            item.source
                        );
                    } else if let Ok(mut list) = current_list.try_lock() {
                        // Filter out same item in current playlist, then add the probe to it.
                        // Check also if duration differs with playlist value, log error if so and adjust that value.
                        for o in list.iter_mut().filter(|o| o.source == item.source) {
                            o.probe.clone_from(&item.probe);

                            if let Some(probe_duration) =
                                item.probe.as_ref().and_then(|f| f.format.duration)
                                && !is_close(o.duration, probe_duration, 1.2)
                            {
                                error!(

                                    channel = id;
                                    "<span class=\"log-gray\">[Validation]</span> File duration (at: <span class=\"log-number\">{}</span>) differs from playlist value. File duration: <span class=\"log-number\">{}</span>, playlist value: <span class=\"log-number\">{}</span>, source <span class=\"log-addr\">{}</span>",
                                    sec_to_time(o.begin.unwrap_or_default()),
                                    sec_to_time(probe_duration),
                                    sec_to_time(o.duration),
                                    o.source
                                );
                                o.duration = probe_duration;
                            }

                            if o.audio == item.audio && item.probe_audio.is_some() {
                                o.probe_audio.clone_from(&item.probe_audio);
                                o.duration_audio = item.duration_audio;
                            }
                        }
                    }
                }
            }

            if config.processing.vtt_enable
                && let Err(e) = check_vtt(&item.source, item.duration, id).await
            {
                error!(channel = id; "{e}");
            }
        }

        begin += item.out - item.seek;
    }

    if !config.playlist.infinit && length > begin + 1.2 {
        error!(channel = id;
            "<span class=\"log-gray\">[Validation]</span> Playlist from <span class=\"log-number\">{date}</span> not long enough, <span class=\"log-number\">{}</span> needed!",
            sec_to_time(length - begin),
        );
    }

    if config.general.validate {
        info!(channel = id;
            "<span class=\"log-gray\">[Validation]</span> Playlist length: <span class=\"log-number\">{}</span>",
            sec_to_time(begin - config.playlist.start_sec.unwrap())
        );
    }

    debug!(channel = id;
        "Validation done, in <span class=\"log-number\">{:.3?}</span>, playlist length: <span class=\"log-number\">{}</span> ...",
        timer.elapsed(),
        sec_to_time(begin - config.playlist.start_sec.unwrap())
    );
}

#[cfg(test)]
mod tests {
    use ff_engine::{EngineAudioStream, EngineMediaProbe, EngineVideoStream, ProbeFormat};

    use super::{Media, av_duration_error};

    fn probe(video_duration: Option<f64>, audio_duration: Option<f64>) -> EngineMediaProbe {
        EngineMediaProbe {
            format: ProbeFormat {
                duration: video_duration.or(audio_duration),
                nb_streams: i64::from(video_duration.is_some())
                    + i64::from(audio_duration.is_some()),
                size: None,
                bit_rate: None,
            },
            video: video_duration
                .map(|duration| EngineVideoStream {
                    aspect_ratio: None,
                    bit_rate: None,
                    codec_name: None,
                    duration: Some(duration),
                    field_order: None,
                    frame_rate: "25/1".to_string(),
                    height: None,
                    nb_frames: None,
                    width: None,
                })
                .into_iter()
                .collect(),
            audio: audio_duration
                .map(|duration| EngineAudioStream {
                    channels: None,
                    codec_name: None,
                    duration: Some(duration),
                    sample_rate: None,
                })
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn accepts_audio_and_video_durations_up_to_four_seconds_apart() {
        let node = Media {
            probe: Some(probe(Some(20.0), Some(16.0))),
            ..Media::default()
        };

        assert!(av_duration_error(&node).is_none());
    }

    #[test]
    fn rejects_audio_and_video_durations_more_than_four_seconds_apart() {
        let node = Media {
            probe: Some(probe(Some(20.001), Some(16.0))),
            ..Media::default()
        };

        let error = av_duration_error(&node).expect("duration mismatch must be rejected");
        assert!(error.contains("more than 4 seconds"));
        assert!(error.contains("difference: 00:00:04.001"));
    }

    #[test]
    fn compares_video_with_the_selected_external_audio() {
        let node = Media {
            source: "video.mp4".to_string(),
            audio: "external.wav".to_string(),
            probe: Some(probe(Some(20.0), Some(20.0))),
            probe_audio: Some(probe(None, Some(10.0))),
            ..Media::default()
        };

        let error = av_duration_error(&node).expect("external audio mismatch must be rejected");
        assert!(error.contains("difference: 00:00:10.000"));
    }

    #[test]
    fn skips_duration_comparison_for_single_stream_media() {
        let video_only = Media {
            probe: Some(probe(Some(20.0), None)),
            ..Media::default()
        };
        let audio_only = Media {
            probe: Some(probe(None, Some(20.0))),
            ..Media::default()
        };

        assert!(av_duration_error(&video_only).is_none());
        assert!(av_duration_error(&audio_only).is_none());
    }
}
