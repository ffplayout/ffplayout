use std::{
    path::{Path, PathBuf},
    process::exit,
};

use regex::Regex;

pub mod arg_parse;

pub use arg_parse::Args;
use ffplayout_lib::{
    filter::Filters,
    utils::{time_to_sec, PlayoutConfig, ProcessMode::*},
    vec_strings,
};

/// Read command line arguments, and override the config with them.
pub fn get_config(args: Args) -> PlayoutConfig {
    let cfg_path = match args.channel {
        Some(c) => {
            let path = PathBuf::from(format!("/etc/ffplayout/{c}.yml"));

            if !path.is_file() {
                println!(
                    "Config file \"{c}\" under \"/etc/ffplayout/\" not found.\n\nCheck arguments!"
                );
                exit(1)
            }

            Some(path.display().to_string())
        }
        None => args.config,
    };

    let mut config = PlayoutConfig::new(cfg_path);

    if let Some(gen) = args.generate {
        config.general.generate = Some(gen);
    }

    if let Some(log_path) = args.log {
        if Path::new(&log_path).is_dir() {
            config.logging.log_to_file = true;
        }
        config.logging.log_path = log_path;
    }

    if let Some(playlist) = args.playlist {
        config.playlist.path = playlist;
    }

    if let Some(mode) = args.play_mode {
        config.processing.mode = mode;
    }

    if let Some(folder) = args.folder {
        config.storage.path = folder;
        config.processing.mode = Folder;
    }

    if let Some(start) = args.start {
        config.playlist.day_start = start.clone();
        config.playlist.start_sec = Some(time_to_sec(&start));
    }

    if let Some(length) = args.length {
        config.playlist.length = length.clone();

        if length.contains(':') {
            config.playlist.length_sec = Some(time_to_sec(&length));
        } else {
            config.playlist.length_sec = Some(86400.0);
        }
    }

    if args.infinit {
        config.playlist.infinit = args.infinit;
    }

    if let Some(output) = args.output {
        config.out.mode = output;
    }

    if let Some(volume) = args.volume {
        config.processing.volume = volume;
    }

    config
}

/// Prepare output parameters
///
/// Seek for multiple outputs and add mapping for it.
pub fn prepare_output_cmd(
    config: &PlayoutConfig,
    mut cmd: Vec<String>,
    filters: &Option<Filters>,
) -> Vec<String> {
    let mut output_params = config.out.clone().output_cmd.unwrap();
    let mut new_params = vec![];
    let mut count = 0;
    let re_v = Regex::new(r"\[?0:v(:0)?\]?").unwrap();

    if let Some(mut filter) = filters.clone() {
        for (i, param) in output_params.iter().enumerate() {
            if filter.video_out_link.len() > count && re_v.is_match(param) {
                new_params.push(filter.video_out_link[count].clone());
            } else {
                new_params.push(param.clone());
            }

            // Check if parameter is a output
            if i > 0
                && !param.starts_with('-')
                && !output_params[i - 1].starts_with('-')
                && i < output_params.len() - 1
            {
                count += 1;

                if filter.video_out_link.len() > count
                    && !output_params.contains(&"-map".to_string())
                {
                    new_params.append(&mut vec_strings![
                        "-map",
                        filter.video_out_link[count].clone()
                    ]);

                    for i in 0..config.processing.audio_tracks {
                        new_params.append(&mut vec_strings!["-map", format!("0:a:{i}")]);
                    }
                }
            }
        }

        output_params = new_params;

        cmd.append(&mut filter.cmd());

        if !filter.map().iter().all(|item| output_params.contains(item))
            && filter.output_chain.is_empty()
            && filter.video_out_link.is_empty()
        {
            cmd.append(&mut filter.map())
        } else if &output_params[0] != "-map" && !filter.video_out_link.is_empty() {
            cmd.append(&mut vec_strings!["-map", filter.video_out_link[0].clone()]);

            for i in 0..config.processing.audio_tracks {
                cmd.append(&mut vec_strings!["-map", format!("0:a:{i}")]);
            }
        }
    }

    cmd.append(&mut output_params);

    cmd
}
