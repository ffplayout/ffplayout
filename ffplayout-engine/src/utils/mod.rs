use std::{
    path::{Path, PathBuf},
    process::exit,
};

use regex::Regex;

pub mod arg_parse;

pub use arg_parse::Args;
use ffplayout_lib::{
    filter::{FilterType::*, Filters},
    utils::{time_to_sec, PlayoutConfig, ProcessMode::*},
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

/// Process filter from output_param
///
/// Split filter string and add them to the existing filtergraph.
fn process_filters(filters: &mut Filters, output_filter: &str) {
    let re_v = Regex::new(r"(\[0:v(:[0-9]+)?\]|\[v[\w_-]+\])").unwrap(); // match video filter links
    let re_a = Regex::new(r"(\[0:a(:[0-9]+)?\]|\[a[\w_-]+\])").unwrap(); // match audio filter links
    let nr = Regex::new(r"\[[\w:-_]+([0-9])\]").unwrap(); // match filter link and get track number

    for f in output_filter.split(';') {
        if re_v.is_match(f) {
            let filter_str = re_v.replace_all(f, "").to_string();
            filters.add_filter(&filter_str, 0, Video);
        } else if re_a.is_match(f) {
            let filter_str = re_a.replace_all(f, "").to_string();
            let track_nr = nr.replace(f, "$1").parse::<i32>().unwrap_or_default();
            filters.add_filter(&filter_str, track_nr, Audio);
        }
    }
}

/// Process filter with multiple in- or output
///
/// Concat filter to the existing filters and adjust filter connections.
/// Output mapping is up to the user.
fn process_multi_in_out(filters: &mut Filters, output_filter: &str) -> Vec<String> {
    let v_map = filters.video_map[filters.video_map.len() - 1].clone();
    let mut new_filter = format!("{}{v_map}", filters.video_chain);
    let re_v = Regex::new(r"\[0:v(:[0-9]+)?\]").unwrap(); // match video input
    let re_a = Regex::new(r"\[0:a(:(?P<num>[0-9]+))?\]").unwrap(); // match audio input, with getting track number
    let mut o_filter = re_v.replace(output_filter, v_map).to_string();

    if !filters.audio_map.is_empty() {
        let a_map = filters.audio_map[filters.audio_map.len() - 1].clone();
        let a_filter = format!("{}{a_map}", filters.audio_chain);

        new_filter.push(';');
        new_filter.push_str(&a_filter);

        o_filter = re_a
            .replace_all(&o_filter, "[aout$num]")
            .to_string()
            .replace("[aout]", "[aout0]"); // when no number matched, set default 0
    }

    new_filter.push(';');
    new_filter.push_str(&o_filter);

    vec!["-filter_complex".to_string(), new_filter]
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
    let mut output_filter = String::new();
    let re_map = Regex::new(r"(\[?[0-9]:[av](:[0-9]+)?\]?|-map$|\[[a-z_0-9]+\])").unwrap(); // match a/v filter links and mapping
    let re_multi = Regex::new(r"\[[\w:_-]+\]\[[\w:_-]+\]").unwrap(); // match multiple filter in/outputs links

    if let Some(mut filter) = filters.clone() {
        // Check if it contains a filtergraph and set correct output mapping.
        for (i, param) in output_params.iter().enumerate() {
            if param != "-filter_complex" {
                if i > 0 && output_params[i - 1] == "-filter_complex" {
                    output_filter = param.clone();
                } else if !re_multi.is_match(&output_filter) {
                    if !re_map.is_match(param)
                        || (i < output_params.len() - 2
                            && (output_params[i + 1].contains("0:s") || param.contains("0:s")))
                    {
                        // Skip mapping parameters, when no multi in/out filter is set.
                        // Only add subtitle mapping.
                        new_params.push(param.clone());
                    }
                } else {
                    new_params.push(param.clone());
                }

                // Check if parameter is a output
                if i > 0
                    && !param.starts_with('-')
                    && !output_params[i - 1].starts_with('-')
                    && i < output_params.len() - 1
                {
                    // add mapping to following outputs
                    new_params.append(&mut filter.map().clone());
                }
            }
        }

        output_params = new_params;

        if re_multi.is_match(&output_filter) {
            let mut split_filter = process_multi_in_out(&mut filter, &output_filter);
            cmd.append(&mut split_filter);
        } else {
            process_filters(&mut filter, &output_filter);
            cmd.append(&mut filter.cmd());
            cmd.append(&mut filter.map());
        }
    }

    cmd.append(&mut output_params);

    cmd
}
