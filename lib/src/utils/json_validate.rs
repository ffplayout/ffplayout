use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use simplelog::*;

use crate::utils::{sec_to_time, valid_source, JsonPlaylist, MediaProbe, PlayoutConfig};

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
            let probe = MediaProbe::new(&item.source);

            if probe.format.is_none() {
                error!(
                    "No Metadata at <yellow>{}</>, from file <b><magenta>{}</></b>",
                    sec_to_time(begin),
                    item.source
                );
            }
        } else {
            error!(
                "Source on position <yellow>{}</> not exists: <b><magenta>{}</></b>",
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
