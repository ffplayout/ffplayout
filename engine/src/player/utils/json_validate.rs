use std::{
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

use log::*;
use regex::Regex;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Mutex,
};

use crate::player::filter::FilterType::Audio;
use crate::player::utils::{
    is_close, is_remote, loop_image, sec_to_time, seek_and_length, JsonPlaylist, Media,
};
use crate::utils::{
    config::{OutputMode::Null, PlayoutConfig, FFMPEG_IGNORE_ERRORS, IMAGE_FORMAT},
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
    mut node: Media,
    pos: usize,
    begin: f64,
    config: &PlayoutConfig,
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

    filter.add_filter("silencedetect=n=-30dB", 0, Audio);

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
            "<bright black>[Validator]</> ffmpeg error on position <yellow>{pos}</> - {}: <b><magenta>{}</></b>: {}",
            sec_to_time(begin),
            node.source,
            error_list.join("\n")
        );
    }

    error_list.clear();

    if let Err(e) = enc_proc.wait().await {
        error!(target: Target::file_mail(), channel = id; "Validation process: {e:?}");
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

    length += begin;

    debug!(target: Target::file_mail(), channel = id; "Validate playlist from: <yellow>{date}</>");
    let timer = Instant::now();

    for (index, item) in playlist.program.iter_mut().enumerate() {
        if !is_alive.load(Ordering::SeqCst) {
            return;
        }

        let pos = index + 1;

        if !is_remote(&item.source) {
            if item.audio.is_empty() {
                if let Err(e) = item.add_probe(false).await {
                    error!(target: Target::file_mail(), channel = id;
                        "[Validation] Error on position <yellow>{pos:0>3}</> - <yellow>{}</>: {e}",
                        sec_to_time(begin)
                    );
                }
            } else if let Err(e) = item.add_probe(true).await {
                error!(target: Target::file_mail(), channel = id;
                    "[Validation] Error on position <yellow>{pos:0>3}</> - <yellow>{}</>: {e}",
                    sec_to_time(begin)
                );
            }
        }

        if item.probe.is_some() {
            if let Err(e) = check_media(item.clone(), pos, begin, &config).await {
                error!(target: Target::file_mail(), channel = id; "{e}");
            } else if config.general.validate {
                debug!(target: Target::file_mail(), channel = id;
                    "[Validation] Source at <yellow>{}</>, seems fine: <b><magenta>{}</></b>",
                    sec_to_time(begin),
                    item.source
                );
            } else if let Ok(mut list) = current_list.try_lock() {
                // Filter out same item in current playlist, then add the probe to it.
                // Check also if duration differs with playlist value, log error if so and adjust that value.
                list.iter_mut().filter(|list_item| list_item.source == item.source).for_each(|o| {
                    o.probe.clone_from(&item.probe);

                    if let Some(dur) =
                        item.probe.as_ref().and_then(|f| f.format.duration)
                    {
                        let probe_duration = dur;

                        if !is_close(o.duration, probe_duration, 1.2) {
                            error!(target: Target::file_mail(), channel = id;
                                "[Validation] File duration (at: <yellow>{}</>) differs from playlist value. File duration: <yellow>{}</>, playlist value: <yellow>{}</>, source <b><magenta>{}</></b>",
                                sec_to_time(o.begin.unwrap_or_default()), sec_to_time(probe_duration), sec_to_time(o.duration), o.source
                            );

                            o.duration = probe_duration;
                        }
                    }

                    if o.audio == item.audio && item.probe_audio.is_some() {
                        o.probe_audio.clone_from(&item.probe_audio);
                        o.duration_audio = item.duration_audio;
                    }
                });
            }
        }

        begin += item.out - item.seek;
    }

    if !config.playlist.infinit && length > begin + 1.2 {
        error!(target: Target::file_mail(), channel = id;
            "[Validation] Playlist from <yellow>{date}</> not long enough, <yellow>{}</> needed!",
            sec_to_time(length - begin),
        );
    }

    if config.general.validate {
        info!(target: Target::file_mail(), channel = id;
            "[Validation] Playlist length: <yellow>{}</>",
            sec_to_time(begin - config.playlist.start_sec.unwrap())
        );
    }

    debug!(target: Target::file_mail(), channel = id;
        "Validation done, in <yellow>{:.3?}</>, playlist length: <yellow>{}</> ...",
        timer.elapsed(),
        sec_to_time(begin - config.playlist.start_sec.unwrap())
    );
}
