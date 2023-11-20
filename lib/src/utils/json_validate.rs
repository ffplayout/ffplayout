use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use regex::Regex;
use simplelog::*;

use crate::filter::FilterType::Audio;
use crate::utils::{
    errors::ProcError, loop_image, sec_to_time, seek_and_length, valid_source, vec_strings,
    JsonPlaylist, Media, OutputMode::Null, PlayoutConfig, FFMPEG_IGNORE_ERRORS, IMAGE_FORMAT,
};

/// check if ffmpeg can read the file and apply filter to it.
fn check_media(
    mut node: Media,
    pos: usize,
    begin: f64,
    config: &PlayoutConfig,
) -> Result<(), ProcError> {
    let mut enc_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+info"];
    let mut error_list = vec![];
    let mut config = config.clone();
    config.out.mode = Null;

    let mut process_length = 0.1;

    if config.logging.detect_silence {
        process_length = 15.0;
    }

    node.add_probe();

    if node.probe.clone().and_then(|p| p.format).is_none() {
        return Err(ProcError::Custom(format!(
            "No Metadata at position <yellow>{pos}</> {}, from file <b><magenta>\"{}\"</></b>",
            sec_to_time(begin),
            node.source
        )));
    }

    // take care, that no seek and length command is added.
    node.seek = 0.0;
    node.out = node.duration;

    if node
        .source
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .filter(|c| IMAGE_FORMAT.contains(&c.as_str()))
        .is_some()
    {
        node.cmd = Some(loop_image(&node));
    } else {
        node.cmd = Some(seek_and_length(&node));
    }

    node.add_filter(&config, &None);

    let mut filter = node.filter.unwrap_or_default();

    if filter.cmd().len() > 1 {
        let re_clean = Regex::new(r"volume=[0-9.]+")?;

        filter.audio_chain = re_clean
            .replace_all(&filter.audio_chain, "anull")
            .to_string();
    }

    filter.add_filter("silencedetect=n=-30dB", 0, Audio);

    enc_cmd.append(&mut node.cmd.unwrap_or_default());
    enc_cmd.append(&mut filter.cmd());
    enc_cmd.append(&mut filter.map());
    enc_cmd.append(&mut vec_strings!["-t", process_length, "-f", "null", "-"]);

    let mut enc_proc = Command::new("ffmpeg")
        .args(enc_cmd)
        .stderr(Stdio::piped())
        .spawn()?;

    let enc_err = BufReader::new(enc_proc.stderr.take().unwrap());
    let mut silence_start = 0.0;
    let mut silence_end = 0.0;
    let re_start = Regex::new(r"silence_start: ([0-9]+:)?([0-9.]+)")?;
    let re_end = Regex::new(r"silence_end: ([0-9]+:)?([0-9.]+)")?;

    for line in enc_err.lines() {
        let line = line?;

        if !FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i))
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
        error!(
            "<bright black>[Validator]</> ffmpeg error on position <yellow>{pos}</> - {}: <b><magenta>{}</></b>: {}",
            sec_to_time(begin),
            node.source,
            error_list.join("\n")
        )
    }

    error_list.clear();

    if let Err(e) = enc_proc.wait() {
        error!("Validation process: {e:?}");
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
pub fn validate_playlist(
    playlist: JsonPlaylist,
    is_terminated: Arc<AtomicBool>,
    mut config: PlayoutConfig,
) {
    let date = playlist.date;

    if config.text.add_text && !config.text.text_from_filename {
        // Turn of drawtext filter with zmq, because its port is needed by the decoder instance.
        config.text.add_text = false;
    }

    let mut length = config.playlist.length_sec.unwrap();
    let mut begin = config.playlist.start_sec.unwrap();

    length += begin;

    debug!("Validate playlist from: <yellow>{date}</>");

    for (index, item) in playlist.program.iter().enumerate() {
        if is_terminated.load(Ordering::SeqCst) {
            return;
        }

        let pos = index + 1;

        if valid_source(&item.source) {
            if let Err(e) = check_media(item.clone(), pos, begin, &config) {
                error!("{e}");
            } else if config.general.validate {
                debug!(
                    "Source at <yellow>{}</>, seems fine: <b><magenta>{}</></b>",
                    sec_to_time(begin),
                    item.source
                )
            };
        } else {
            error!(
                "Source on position <yellow>{pos:0>3}</> {} not exists: <b><magenta>{}</></b>",
                sec_to_time(begin),
                item.source
            );
        }

        begin += item.out - item.seek;
    }

    if !config.playlist.infinit && length > begin + 1.2 {
        error!(
            "Playlist from <yellow>{date}</> not long enough, <yellow>{}</> needed!",
            sec_to_time(length - begin),
        );
    }

    debug!("Validation done...");
}
