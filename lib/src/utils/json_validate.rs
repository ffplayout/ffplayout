use std::{
    io::{BufRead, BufReader, Error, ErrorKind},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use simplelog::*;

use crate::utils::{
    format_log_line, loop_image, sec_to_time, seek_and_length, valid_source, vec_strings,
    JsonPlaylist, Media, PlayoutConfig, FFMPEG_IGNORE_ERRORS, IMAGE_FORMAT,
};

/// check if ffmpeg can read the file and apply filter to it.
fn check_media(
    mut node: Media,
    pos: usize,
    begin: f64,
    config: &PlayoutConfig,
) -> Result<(), Error> {
    let mut enc_cmd = vec_strings!["-hide_banner", "-nostats", "-v", "level+error"];
    let mut error_list = vec![];

    node.add_probe();

    if node.probe.clone().and_then(|p| p.format).is_none() {
        return Err(Error::new(
            ErrorKind::Other,
            format!(
                "No Metadata at position <yellow>{pos}</> {}, from file <b><magenta>\"{}\"</></b>",
                sec_to_time(begin),
                node.source
            ),
        ));
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

    node.add_filter(config, &None);

    let mut filter = node.filter.unwrap_or_default();

    if filter.cmd.len() > 1 {
        filter.cmd[1] = filter.cmd[1].replace("realtime=speed=1", "null")
    }

    enc_cmd.append(&mut node.cmd.unwrap_or_default());
    enc_cmd.append(&mut filter.cmd);
    enc_cmd.append(&mut vec_strings!["-t", "0.1", "-f", "null", "-"]);

    let mut enc_proc = match Command::new("ffmpeg")
        .args(enc_cmd.clone())
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => return Err(e),
        Ok(proc) => proc,
    };

    let enc_err = BufReader::new(enc_proc.stderr.take().unwrap());

    for line in enc_err.lines() {
        let line = line?;

        if !FFMPEG_IGNORE_ERRORS.iter().any(|i| line.contains(*i)) {
            if line.contains("[error]") {
                let log_line = format_log_line(line, "error");

                if !error_list.contains(&log_line) {
                    error_list.push(log_line);
                }
            } else if line.contains("[fatal]") {
                let log_line = format_log_line(line, "fatal");

                if !error_list.contains(&log_line) {
                    error_list.push(log_line);
                }
            }
        }
    }

    if !error_list.is_empty() {
        error!(
            "<bright black>[Validator]</> ffmpeg error on position <yellow>{pos}</> - {}: <b><magenta>{}</></b>:\n{}",
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
            };
        } else {
            error!(
                "Source on position <yellow>{pos}</> {} not exists: <b><magenta>\"{}\"</></b>",
                sec_to_time(begin),
                item.source
            );
        }

        begin += item.out - item.seek;
    }

    if !config.playlist.infinit && length > begin + 1.0 {
        error!(
            "Playlist from <yellow>{date}</> not long enough, <yellow>{}</> needed!",
            sec_to_time(length - begin),
        );
    }

    debug!("Validation done...");
}
