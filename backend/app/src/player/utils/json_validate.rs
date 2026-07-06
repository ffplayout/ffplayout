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
        JsonPlaylist, Media, is_close, is_remote, sec_to_time, time_in_seconds, time_to_sec,
    },
    utils::{config::PlayoutConfig, errors::ProcessError},
};

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

    if config.logging.detect_silence {
        debug!(channel = id;
            "<span class=\"log-gray\">[Validation]</span> Silence detection is skipped because app validation no longer starts the ffmpeg binary."
        );
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
                error!(channel = channel_id;
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
    mut config: PlayoutConfig,
    current_list: Arc<Mutex<Vec<Media>>>,
    mut playlist: JsonPlaylist,
    is_alive: Arc<AtomicBool>,
    cancel_token: CancellationToken,
) {
    let id = config.general.channel_id;
    let date = playlist.date;

    if config.text.add_text && !config.text.text_from_filename {
        // Turn of drawtext filter with zmq, because its port is needed by the decoder instance.
        config.text.add_text = false;
    }

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
            let source_path = Path::new(&item.source);
            if source_path.is_relative() {
                let new_path = config.storage.path.join(source_path);
                item.source = new_path.to_string_lossy().to_string();
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
