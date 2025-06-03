use std::{
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

use log::*;
use regex::Regex;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Mutex,
};

use crate::player::filter::FilterType::Audio;
use crate::player::utils::{
    JsonPlaylist, Media, is_close, is_remote, loop_image, sec_to_time, seek_and_length,
    time_in_seconds, time_to_sec,
};
use crate::utils::{
    config::{FFMPEG_IGNORE_ERRORS, IMAGE_FORMAT, OutputMode::Null, PlayoutConfig},
    errors::ProcessError,
    logging::Target,
};
use crate::vec_strings;

/// Validate a single media file.
///
/// - Check if file exists
/// - Check if ffmpeg can read the file
/// - Check if Metadata exists
/// - Check if the file is not silent
async fn check_media(
    config: &PlayoutConfig,
    mut node: Media,
    pos: usize,
    begin: f64,
) -> Result<(), ProcessError> {
    let id = config.general.channel_id;
    let mut dec_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let mut error_list = vec![];
    let mut config = config.clone();
    config.output.mode = Null;

    let mut process_length = 0.1;

    if let Some(decoder_input_cmd) = &config.advanced.decoder.input_cmd {
        dec_cmd.append(&mut decoder_input_cmd.clone());
    }

    if config.logging.detect_silence {
        process_length = 15.0;
        let seek = node.duration / 4.0;

        // Seek in file, to prevent false silence detection on intros without sound.
        dec_cmd.append(&mut vec_strings!["-ss", seek]);
    }

    // Take care, that no seek and length command is added.
    node.seek = 0.0;
    node.out = node.duration;

    if node
        .source
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .filter(|c| IMAGE_FORMAT.contains(&c.as_str()))
        .is_some()
    {
        node.cmd = Some(loop_image(&config, &node));
    } else {
        node.cmd = Some(seek_and_length(&config, &mut node));
    }

    node.add_filter(&config, &None).await;

    let mut filter = node.filter.unwrap_or_default();

    if filter.cmd().len() > 1 {
        let re_clean = Regex::new(r"volume=[0-9.]+")?;

        filter.audio_chain = re_clean
            .replace_all(&filter.audio_chain, "anull")
            .to_string();
    }

    filter.add("silencedetect=n=-30dB", 0, Audio);

    dec_cmd.append(&mut node.cmd.unwrap_or_default());
    dec_cmd.append(&mut filter.cmd());
    dec_cmd.append(&mut filter.map());
    dec_cmd.append(&mut vec_strings!["-t", process_length, "-f", "null", "-"]);

    let mut enc_proc = Command::new("ffmpeg")
        .args(dec_cmd)
        .kill_on_drop(true)
        .stderr(Stdio::piped())
        .spawn()?;

    let enc_err = BufReader::new(enc_proc.stderr.take().unwrap());
    let mut lines = enc_err.lines();
    let mut silence_start = 0.0;
    let mut silence_end = 0.0;
    let re_start = Regex::new(r"silence_start: ([0-9]+:)?([0-9.]+)")?;
    let re_end = Regex::new(r"silence_end: ([0-9]+:)?([0-9.]+)")?;

    while let Some(line) = lines.next_line().await? {
        if !FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i))
            && !config.logging.ignore_lines.iter().any(|i| line.contains(i))
            && (line.contains("[error]") || line.contains("[fatal]"))
        {
            let log_line = line.replace("[error] ", "").replace("[fatal] ", "");

            if !error_list.contains(&log_line) {
                error_list.push(log_line);
            }
        }

        if config.logging.detect_silence {
            if let Some(start) = re_start.captures(&line).and_then(|c| c.get(2)) {
                silence_start = start.as_str().parse::<f32>().unwrap_or_default();
            }

            if let Some(end) = re_end.captures(&line).and_then(|c| c.get(2)) {
                silence_end = end.as_str().parse::<f32>().unwrap_or_default() + 0.5;
            }
        }
    }

    if silence_end - silence_start > process_length {
        error_list.push("Audio is totally silent!".to_string());
    }

    if !error_list.is_empty() {
        error!(target: Target::file_mail(), channel = id;
            "<span class=\"log-gray\">[Validator]</span> ffmpeg error on position <span class=\"log-number\">{pos}</span> - {}: <span class=\"log-addr\">{}</span>: {}",
            sec_to_time(begin),
            node.source,
            error_list.join("\n    ")
        );
    }

    error_list.clear();

    if let Err(e) = enc_proc.wait().await {
        error!(target: Target::file_mail(), channel = id; "Validation process: {e:?}");
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
                error!(target: Target::file_mail(), channel = channel_id;
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
/// - file can be read by ffprobe and metadata exists
/// - total playtime fits target length from config
///
/// This function we run in a thread, to don't block the main function.
pub async fn validate_playlist(
    mut config: PlayoutConfig,
    current_list: Arc<Mutex<Vec<Media>>>,
    mut playlist: JsonPlaylist,
    is_alive: Arc<AtomicBool>,
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

    debug!(target: Target::file_mail(), channel = id; "Validate playlist from: <span class=\"log-number\">{date}</span>");
    let timer = Instant::now();

    for (index, item) in playlist.program.iter_mut().enumerate() {
        if !is_alive.load(Ordering::SeqCst) {
            return;
        }

        let pos = index + 1;

        if begin < time_sec {
            // Do not validate clips that are being passed.
            begin += item.out - item.seek;
            continue;
        }

        if !is_remote(&item.source) {
            if item.audio.is_empty() {
                if let Err(e) = item.add_probe(false).await {
                    error!(target: Target::file_mail(), channel = id;
                        "<span class=\"log-gray\">[Validation]</span> Error on position <span class=\"log-number\">{pos:0>3}</span> - <span class=\"log-number\">{}</span>: {e}",
                        sec_to_time(begin)
                    );
                }
            } else if let Err(e) = item.add_probe(true).await {
                error!(target: Target::file_mail(), channel = id;
                    "<span class=\"log-gray\">[Validation]</span> Error on position <span class=\"log-number\">{pos:0>3}</span> - <span class=\"log-number\">{}</span>: {e}",
                    sec_to_time(begin)
                );
            }
        }

        if item.probe.is_some() {
            match check_media(&config, item.clone(), pos, begin).await {
                Err(e) => {
                    error!(target: Target::file_mail(), channel = id; "{e}");
                }
                Ok(()) => {
                    if config.general.validate {
                        debug!(
                            target: Target::file_mail(), channel = id;
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
                            {
                                if !is_close(o.duration, probe_duration, 1.2) {
                                    error!(
                                        target: Target::file_mail(),
                                        channel = id;
                                        "<span class=\"log-gray\">[Validation]</span> File duration (at: <span class=\"log-number\">{}</span>) differs from playlist value. File duration: <span class=\"log-number\">{}</span>, playlist value: <span class=\"log-number\">{}</span>, source <span class=\"log-addr\">{}</span>",
                                        sec_to_time(o.begin.unwrap_or_default()),
                                        sec_to_time(probe_duration),
                                        sec_to_time(o.duration),
                                        o.source
                                    );
                                    o.duration = probe_duration;
                                }
                            }

                            if o.audio == item.audio && item.probe_audio.is_some() {
                                o.probe_audio.clone_from(&item.probe_audio);
                                o.duration_audio = item.duration_audio;
                            }
                        }
                    }
                }
            }

            if config.processing.vtt_enable {
                if let Err(e) = check_vtt(&item.source, item.duration, id).await {
                    error!(target: Target::file_mail(), channel = id; "{e}");
                }
            }
        }

        begin += item.out - item.seek;
    }

    if !config.playlist.infinit && length > begin + 1.2 {
        error!(target: Target::file_mail(), channel = id;
            "<span class=\"log-gray\">[Validation]</span> Playlist from <span class=\"log-number\">{date}</span> not long enough, <span class=\"log-number\">{}</span> needed!",
            sec_to_time(length - begin),
        );
    }

    if config.general.validate {
        info!(target: Target::file_mail(), channel = id;
            "<span class=\"log-gray\">[Validation]</span> Playlist length: <span class=\"log-number\">{}</span>",
            sec_to_time(begin - config.playlist.start_sec.unwrap())
        );
    }

    debug!(target: Target::file_mail(), channel = id;
        "Validation done, in <span class=\"log-number\">{:.3?}</span>, playlist length: <span class=\"log-number\">{}</span> ...",
        timer.elapsed(),
        sec_to_time(begin - config.playlist.start_sec.unwrap())
    );
}
