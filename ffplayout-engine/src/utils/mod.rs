use std::{
    path::{Path, PathBuf},
    process::exit,
};

use regex::Regex;

pub mod arg_parse;

pub use arg_parse::Args;
use ffplayout_lib::{
    filter::Filters,
    utils::{time_to_sec, OutputMode::*, PlayoutConfig, ProcessMode::*},
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
/// seek for multiple outputs and add mapping for it
pub fn prepare_output_cmd(
    mut cmd: Vec<String>,
    filters: &Option<Filters>,
    config: &PlayoutConfig,
) -> Vec<String> {
    let mut output_params = config.out.clone().output_cmd.unwrap();
    let params_len = output_params.len();
    let mut new_params = vec![];
    let mut output_filter = String::new();
    let mut next_is_filter = false;
    let re_map = Regex::new(r"(\[?[0-9]:[av](:[0-9]+)?\]?|-map|\[[a-z_0-9]+\])").unwrap();

    if let Some(mut filter) = filters.clone() {
        // Loop over output parameters
        //
        // Check if it contains a filtergraph, count its outputs and set correct mapping values.
        for (i, param) in output_params.iter().enumerate() {
            // Skip filter command, to concat existing filters with new ones.
            if param != "-filter_complex" {
                if next_is_filter {
                    output_filter = param.clone();
                    next_is_filter = false;
                } else if !output_filter.contains("split") {
                    if !re_map.is_match(param) {
                        // Skip mapping parameters, when no split filter is set
                        new_params.push(param.clone());
                    }
                } else {
                    new_params.push(param.clone());
                }
            } else {
                next_is_filter = true;
            }

            // Check if parameter is a output
            if i > 0
                && !param.starts_with('-')
                && !output_params[i - 1].starts_with('-')
                && i < params_len - 1
            {
                new_params.append(&mut filter.output_map.clone());
            }
        }
        if filter.cmd.contains(&"-filter_complex".to_string()) {
            output_params = new_params;

            // Process A/V mapping
            //
            // Check if there is multiple outputs, and/or multiple audio tracks
            // and add the correct mapping for it.
            if !output_filter.is_empty() && config.out.mode == HLS {
                let re = Regex::new(r"0:a:(?P<num>[0-9]+)").unwrap();
                output_filter = re
                    .replace_all(&output_filter, "aout${num}")
                    .to_string()
                    .replace("[0:a]", &filter.audio_map[0])
                    .replace("[0:v]", &filter.video_map[0])
                    .replace("[0:v:0]", &filter.video_map[0]);

                filter.cmd[1].push_str(&format!(";{output_filter}"));
                filter.cmd.drain(2..);
                cmd.append(&mut filter.cmd);
            } else {
                cmd.append(&mut filter.cmd);
            }
        } else {
            cmd.append(&mut filter.cmd);
        }
    }

    cmd.append(&mut output_params);

    cmd
}
