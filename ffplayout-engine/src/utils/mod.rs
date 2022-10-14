use std::{
    path::{Path, PathBuf},
    process::exit,
};

use regex::Regex;

pub mod arg_parse;

pub use arg_parse::Args;
use ffplayout_lib::{
    utils::{time_to_sec, OutputMode::*, PlayoutConfig, ProcessMode::*},
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
/// seek for multiple outputs and add mapping for it
pub fn prepare_output_cmd(
    mut cmd: Vec<String>,
    mut filter: Vec<String>,
    config: &PlayoutConfig,
) -> Vec<String> {
    let mut output_params = config.out.clone().output_cmd.unwrap();
    let mut new_params = vec![];
    let params_len = output_params.len();
    let mut output_a_map = "[a_out1]".to_string();
    let mut output_v_map = "[v_out1]".to_string();
    let mut out_count = 1;
    let mut output_filter = String::new();
    let mut next_is_filter = false;
    let re_audio_map = Regex::new(r"\[0:a:(?P<num>[0-9]+)\]").unwrap();

    // Loop over output parameters
    //
    // Check if it contains a filtergraph, count its outputs and set correct mapping values.
    for (i, p) in output_params.iter().enumerate() {
        let mut param = p.clone();

        param = param.replace("[0:v]", "[vout0]");
        param = param.replace("[0:a]", "[aout0]");
        param = re_audio_map.replace_all(&param, "[aout$num]").to_string();

        // Skip filter command, to concat existing filters with new ones.
        if param != "-filter_complex" {
            if next_is_filter {
                output_filter = param.clone();
                next_is_filter = false;
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
            out_count += 1;
            let mut a_map = "0:a".to_string();
            let v_map = format!("[v_out{out_count}]");
            output_v_map.push_str(&v_map);

            if config.out.mode == HLS {
                a_map = format!("[a_out{out_count}]");
            }

            output_a_map.push_str(&a_map);

            if !output_params.contains(&"-map".to_string()) {
                let mut map = vec_strings!["-map", v_map, "-map", a_map];
                new_params.append(&mut map);
            }
        }
    }

    if !filter.is_empty() {
        output_params = new_params;

        // Process A/V mapping
        //
        // Check if there is multiple outputs, and/or multiple audio tracks
        // and add the correct mapping for it.
        if out_count > 1 && config.processing.audio_tracks == 1 && config.out.mode == HLS {
            filter[1].push_str(&format!(";[vout0]split={out_count}{output_v_map}"));
            filter[1].push_str(&format!(";[aout0]asplit={out_count}{output_a_map}"));
            filter.drain(2..);
            cmd.append(&mut filter);
            cmd.append(&mut vec_strings!["-map", "[v_out1]", "-map", "[a_out1]"]);
        } else if !output_filter.is_empty() && config.out.mode == HLS {
            filter[1].push_str(&format!(";{output_filter}"));
            filter.drain(2..);
            cmd.append(&mut filter);
        } else if out_count == 1
            && config.processing.audio_tracks == 1
            && config.out.mode == HLS
            && output_params[0].contains("split")
        {
            let out_filter = output_params.remove(0);
            filter[1].push_str(&format!(";{out_filter}"));
            filter.drain(2..);
            cmd.append(&mut filter);
        } else if out_count > 1 && config.processing.audio_tracks == 1 && config.out.mode == Stream
        {
            filter[1].push_str(&format!(",split={out_count}{output_v_map}"));
            cmd.append(&mut filter);
            cmd.append(&mut vec_strings!["-map", "[v_out1]", "-map", "0:a"]);
        } else if config.processing.audio_tracks > 1 && config.out.mode == Stream {
            filter[1].push_str("[v_out1]");
            cmd.append(&mut filter);

            output_params = output_params
                .iter()
                .map(|p| p.replace("0:v", "[v_out1]"))
                .collect();

            if out_count == 1 {
                cmd.append(&mut vec_strings!["-map", "[v_out1]"]);

                for i in 0..config.processing.audio_tracks {
                    cmd.append(&mut vec_strings!["-map", format!("0:a:{i}")]);
                }
            }
        } else {
            cmd.append(&mut filter);
        }
    } else if out_count == 1 && config.processing.audio_tracks > 1 && config.out.mode == Stream {
        cmd.append(&mut vec_strings!["-map", "0:v"]);

        for i in 0..config.processing.audio_tracks {
            cmd.append(&mut vec_strings!["-map", format!("0:a:{i}")]);
        }
    }

    cmd.append(&mut output_params);

    cmd
}
