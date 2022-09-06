use std::{
    io::{BufRead, BufReader, Error, ErrorKind},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use simplelog::*;

use crate::utils::{
    format_log_line, sec_to_time, valid_source, vec_strings, Media, JsonPlaylist, PlayoutConfig,
};

/// check if ffmpeg can read the file and apply filter to it.
fn check_media(item: Media, begin: f64, config: &PlayoutConfig) -> Result<(), Error> {
    let mut clip = item;
    clip.add_probe();
    clip.add_filter(config, &Arc::new(Mutex::new(vec![])));

    let enc_cmd = vec_strings![
        "-hide_banner",
        "-nostats",
        "-v",
        "level+error",
        "-ignore_chapters",
        "1",
        "-i",
        clip.source,
        "-t",
        "0.25",
        "-f",
        "null",
        "-"
    ];

    if clip.probe.and_then(|p| p.format).is_none() {
        return Err(Error::new(
            ErrorKind::Other,
            format!(
                "No Metadata at <yellow>{}</>, from file <b><magenta>\"{}\"</></b>",
                sec_to_time(begin),
                clip.source
            ),
        ));
    }

    let mut enc_proc = match Command::new("ffmpeg")
        .args(enc_cmd)
        .stderr(Stdio::piped())
        .spawn()
    {
        Err(e) => return Err(e),
        Ok(proc) => proc,
    };

    let enc_err = BufReader::new(enc_proc.stderr.take().unwrap());

    for line in enc_err.lines() {
        let line = line?;

        if line.contains("[error]") {
            error!(
                "<bright black>[Validator]</> {}",
                format_log_line(line, "error")
            );
        } else if line.contains("[fatal]") {
            error!(
                "<bright black>[Validator]</> {}",
                format_log_line(line, "fatal")
            )
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
pub fn validate_playlist(
    playlist: JsonPlaylist,
    is_terminated: Arc<AtomicBool>,
    config: PlayoutConfig,
) {
    let date = playlist.date;
    let mut length = config.playlist.length_sec.unwrap();
    let mut begin = config.playlist.start_sec.unwrap();

    length += begin;

    debug!("validate playlist from: <yellow>{date}</>");

    for item in playlist.program.iter() {
        if is_terminated.load(Ordering::SeqCst) {
            return;
        }

        if valid_source(&item.source) {
            if let Err(e) = check_media(item.clone(), begin, &config) {
                error!("{e}");
            };
        } else {
            error!(
                "Source on position <yellow>{}</> not exists: <b><magenta>\"{}\"</></b>",
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
}
