/*
This module write the files compression directly to a hls (m3u8) playlist,
without pre- and post-processing.

Example config:

out:
    output_param: >-
        ...

        -flags +cgop
        -f hls
        -hls_time 6
        -hls_list_size 600
        -hls_flags append_list+delete_segments+omit_endlist+program_date_time
        -hls_segment_filename /var/www/html/live/stream-%09d.ts /var/www/html/live/stream.m3u8

*/

use std::process::{Command, Stdio};

use simplelog::*;
use tokio::runtime::Handle;

use crate::output::source_generator;
use crate::utils::{
    sec_to_time, stderr_reader, GlobalConfig, PlayerControl, PlayoutStatus, ProcessControl,
};

pub fn write_hls(
    rt_handle: &Handle,
    play_control: PlayerControl,
    playout_stat: PlayoutStatus,
    proc_control: ProcessControl,
) {
    let config = GlobalConfig::global();
    let dec_settings = config.out.clone().output_cmd.unwrap();
    let ff_log_format = format!("level+{}", config.logging.ffmpeg_level.to_lowercase());

    let get_source = source_generator(
        rt_handle,
        config.clone(),
        play_control.current_list.clone(),
        play_control.index.clone(),
        playout_stat,
        proc_control.is_terminated.clone(),
    );

    for node in get_source {
        *play_control.current_media.lock().unwrap() = Some(node.clone());

        let cmd = match node.cmd {
            Some(cmd) => cmd,
            None => break,
        };

        if !node.process.unwrap() {
            continue;
        }

        info!(
            "Play for <yellow>{}</>: <b><magenta>{}</></b>",
            sec_to_time(node.out - node.seek),
            node.source
        );

        let filter = node.filter.unwrap();
        let mut dec_cmd = vec!["-hide_banner", "-nostats", "-v", ff_log_format.as_str()];
        dec_cmd.append(&mut cmd.iter().map(String::as_str).collect());

        if filter.len() > 1 {
            dec_cmd.append(&mut filter.iter().map(String::as_str).collect());
        }

        dec_cmd.append(&mut dec_settings.iter().map(String::as_str).collect());

        debug!(
            "HLS writer CMD: <bright-blue>\"ffmpeg {}\"</>",
            dec_cmd.join(" ")
        );

        let mut dec_proc = match Command::new("ffmpeg")
            .args(dec_cmd)
            .stderr(Stdio::piped())
            .spawn()
        {
            Err(e) => {
                error!("couldn't spawn decoder process: {e}");
                panic!("couldn't spawn decoder process: {e}")
            }
            Ok(proc) => proc,
        };

        rt_handle.spawn(stderr_reader(
            dec_proc.stderr.take().unwrap(),
            "Writer",
        ));

        if let Err(e) = dec_proc.wait() {
            error!("Writer: {e}")
        };
    }
}
